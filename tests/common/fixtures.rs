use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_test_creator(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO creators (id, username, wallet_address, email, created_at) VALUES ($1, $2, $3, $4, NOW())"
    )
    .bind(id)
    .bind(username)
    .bind("GTEST_WALLET")
    .bind(format!("{}@example.com", username))
    .execute(pool)
    .await
    .unwrap();
    id
}

pub async fn create_test_tip(pool: &PgPool, username: &str, amount: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tips (id, creator_username, amount, transaction_hash, created_at) VALUES ($1, $2, $3, $4, NOW())"
    )
    .bind(id)
    .bind(username)
    .bind(amount)
    .bind(format!("TX_{}", Uuid::new_v4()))
    .execute(pool)
    .await
    .unwrap();
    id
}
