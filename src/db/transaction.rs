use sqlx::{Postgres, Transaction};
use anyhow::Result;

/// A helper to begin a new database transaction.
/// Rolls back automatically on drop if not committed.
pub async fn begin_transaction(pool: &sqlx::PgPool) -> Result<Transaction<'_, Postgres>> {
    pool.begin().await.map_err(Into::into)
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

    #[tokio::test]
    async fn transaction_rollback_works() {
        let pool = get_test_pool().await;
        
        let mut tx = begin_transaction(&pool).await.unwrap();
        sqlx::query("INSERT INTO creators (username, wallet_address) VALUES ('rollback_test', 'abc')")
            .execute(&mut *tx)
            .await
            .unwrap();
        
        // Explicitly drop without commit (should rollback)
        drop(tx);
        
        let found = sqlx::query("SELECT 1 FROM creators WHERE username = 'rollback_test'")
            .fetch_optional(&pool)
            .await
            .unwrap();
        
        assert!(found.is_none(), "Identity should not have been persisted after rollback");
    }

    #[tokio::test]
    async fn savepoint_recovery_works() {
        let pool = get_test_pool().await;
        let mut tx = begin_transaction(&pool).await.unwrap();

        // 1. Successful insert
        sqlx::query("INSERT INTO creators (username, wallet_address) VALUES ('p1', 'addr1')")
            .execute(&mut *tx)
            .await
            .unwrap();

        // 2. Start savepoint
        create_savepoint(&mut tx, "sp1").await.unwrap();
        
        // 3. Failing insert (duplicate username if we used 'p1' again, but let's just use bad SQL)
        let res = sqlx::query("INSERT INTO creators (username, wallet_address) VALUES ('p1', 'addr1')")
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
        
        assert_eq!(count.0, 1, "Only the first insert should have been committed");
        
        // Cleanup
        sqlx::query("DELETE FROM creators WHERE username = 'p1'").execute(&pool).await.unwrap();
    }
}
