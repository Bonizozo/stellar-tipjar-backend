use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_test_creator(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO creators (id, username, wallet_address, email, created_at) \
         VALUES ($1, $2, $3, $4, NOW()) ON CONFLICT (username) DO NOTHING"
    )
    .bind(id)
    .bind(username)
    .bind("GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5")
    .bind(format!("{}@example.com", username))
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Insert a confirmed tip directly (bypasses verification pipeline).
pub async fn create_test_tip(pool: &PgPool, username: &str, amount: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tips \
         (id, creator_username, amount, transaction_hash, tipper_source_account, status, created_at) \
         VALUES ($1, $2, $3, $4, $5, 'confirmed', NOW())"
    )
    .bind(id)
    .bind(username)
    .bind(amount)
    .bind(format!("{:0>64}", Uuid::new_v4().simple()))
    .bind("GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN")
    .execute(pool)
    .await
    .unwrap();
    id
}
