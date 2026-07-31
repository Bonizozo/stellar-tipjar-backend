use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};
use std::future::Future;

/// A helper to begin a new database transaction.
/// Rolls back automatically on drop if not committed.
pub async fn begin_transaction(pool: &sqlx::PgPool) -> Result<Transaction<'_, Postgres>> {
    pool.begin().await.map_err(Into::into)
}

/// Runs `f` inside a transaction, committing on `Ok` and rolling back on `Err`.
pub async fn with_transaction<F, T, E>(pool: &PgPool, f: F) -> std::result::Result<T, E>
where
    F: for<'a> FnOnce(
        &'a mut Transaction<'_, Postgres>,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = std::result::Result<T, E>> + Send + 'a>,
    >,
    E: From<sqlx::Error>,
{
    let mut tx = pool.begin().await.map_err(E::from)?;
    match f(&mut tx).await {
        Ok(result) => {
            tx.commit().await.map_err(E::from)?;
            Ok(result)
        }
        Err(e) => {
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

/// Like `with_transaction` but sets `SERIALIZABLE` isolation before running `f`.
pub async fn with_serializable_transaction<F, T, E>(
    pool: &PgPool,
    f: F,
) -> std::result::Result<T, E>
where
    F: for<'a> FnOnce(
        &'a mut Transaction<'_, Postgres>,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = std::result::Result<T, E>> + Send + 'a>,
    >,
    E: From<sqlx::Error>,
{
    let mut tx = pool.begin().await.map_err(E::from)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await
        .map_err(E::from)?;
    match f(&mut tx).await {
        Ok(result) => {
            tx.commit().await.map_err(E::from)?;
            Ok(result)
        }
        Err(e) => {
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

/// Creates a new savepoint within an existing transaction to provide
/// a form of nested transaction support as required by PostgreSQL.
pub async fn create_savepoint(tx: &mut Transaction<'_, Postgres>, name: &str) -> Result<()> {
    sqlx::query(&format!("SAVEPOINT {}", name))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Rolls back to a specific savepoint, allowing error recovery from a
/// partial operation failure without aborting the entire transaction.
pub async fn rollback_savepoint(tx: &mut Transaction<'_, Postgres>, name: &str) -> Result<()> {
    sqlx::query(&format!("ROLLBACK TO SAVEPOINT {}", name))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Explicitly releases a savepoint once it's no longer needed.
pub async fn release_savepoint(tx: &mut Transaction<'_, Postgres>, name: &str) -> Result<()> {
    sqlx::query(&format!("RELEASE SAVEPOINT {}", name))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use std::env;

    async fn get_test_pool() -> sqlx::PgPool {
        dotenvy::dotenv().ok();
        let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .expect("Failed to connect to test database")
    }

    /// `creators.tenant_id` is a mandatory FK with no valid default (its
    /// `DEFAULT gen_random_uuid()` never matches an existing row), so every
    /// raw insert in these tests needs a real tenant row to point at.
    /// `name_prefix` gets a UUID suffix so repeated runs against the same
    /// database don't collide on `tenants.name`'s unique constraint.
    async fn insert_test_tenant(pool: &sqlx::PgPool, name_prefix: &str) -> uuid::Uuid {
        let name = format!("{name_prefix}_{}", uuid::Uuid::new_v4());
        sqlx::query_scalar::<_, uuid::Uuid>("INSERT INTO tenants (name) VALUES ($1) RETURNING id")
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn transaction_rollback_works() {
        let pool = get_test_pool().await;
        let tenant_id = insert_test_tenant(&pool, "rollback_test_tenant").await;

        let mut tx = begin_transaction(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO creators (username, wallet_address, tenant_id) VALUES ('rollback_test', 'abc', $1)",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Explicitly drop without commit (should rollback)
        drop(tx);

        let found = sqlx::query("SELECT 1 FROM creators WHERE username = 'rollback_test'")
            .fetch_optional(&pool)
            .await
            .unwrap();

        assert!(
            found.is_none(),
            "Identity should not have been persisted after rollback"
        );
    }

    #[tokio::test]
    async fn savepoint_recovery_works() {
        let pool = get_test_pool().await;
        let tenant_id = insert_test_tenant(&pool, "savepoint_test_tenant").await;
        let mut tx = begin_transaction(&pool).await.unwrap();

        // 1. Successful insert
        sqlx::query(
            "INSERT INTO creators (username, wallet_address, tenant_id) VALUES ('p1', 'addr1', $1)",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .unwrap();

        // 2. Start savepoint
        create_savepoint(&mut tx, "sp1").await.unwrap();

        // 3. Failing insert (duplicate username if we used 'p1' again, but let's just use bad SQL)
        let res = sqlx::query(
            "INSERT INTO creators (username, wallet_address, tenant_id) VALUES ('p1', 'addr1', $1)",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await;

        assert!(res.is_err(), "Should fail due to unique constraint");

        // 4. Recover via rollback to savepoint
        rollback_savepoint(&mut tx, "sp1").await.unwrap();

        // 5. Commit remaining transaction
        tx.commit().await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM creators WHERE username = 'p1'")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(
            count.0, 1,
            "Only the first insert should have been committed"
        );

        // Cleanup
        sqlx::query("DELETE FROM creators WHERE username = 'p1'")
            .execute(&pool)
            .await
            .unwrap();
    }
}
