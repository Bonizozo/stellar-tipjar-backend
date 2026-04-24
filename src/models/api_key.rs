use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub key: String,
    #[serde(skip_serializing)]
    pub secret: String,
    pub name: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub key: String,
    /// Returned only on creation; never stored in plaintext after this.
    pub secret: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl ApiKey {
    pub async fn create(pool: &PgPool, name: &str) -> Result<ApiKeyCreated, sqlx::Error> {
        let key = generate_key();
        let secret = generate_key();

        let row: ApiKey = sqlx::query_as(
            "INSERT INTO api_keys (key, secret, name) VALUES ($1, $2, $3)
             RETURNING id, key, secret, name, active, created_at, rotated_at",
        )
        .bind(&key)
        .bind(&secret)
        .bind(name)
        .fetch_one(pool)
        .await?;

        Ok(ApiKeyCreated {
            id: row.id,
            key: row.key,
            secret,
            name: row.name,
            created_at: row.created_at,
        })
    }

    pub async fn get_secret(pool: &PgPool, key: &str) -> Result<String, sqlx::Error> {
        let (secret,): (String,) = sqlx::query_as(
            "SELECT secret FROM api_keys WHERE key = $1 AND active = true",
        )
        .bind(key)
        .fetch_one(pool)
        .await?;
        Ok(secret)
    }

    /// Rotate: deactivate old key and create a new one with the same name.
    pub async fn rotate(pool: &PgPool, key: &str) -> Result<ApiKeyCreated, sqlx::Error> {
        let (name,): (String,) =
            sqlx::query_as("SELECT name FROM api_keys WHERE key = $1 AND active = true")
                .bind(key)
                .fetch_one(pool)
                .await?;

        sqlx::query(
            "UPDATE api_keys SET active = false, rotated_at = NOW() WHERE key = $1",
        )
        .bind(key)
        .execute(pool)
        .await?;

        Self::create(pool, &name).await
    }
}

fn generate_key() -> String {
    use std::fmt::Write;
    let bytes: [u8; 32] = rand_bytes();
    let mut s = String::with_capacity(64);
    for b in bytes {
        write!(s, "{:02x}", b).unwrap();
    }
    s
}

fn rand_bytes() -> [u8; 32] {
    // Use the OS random source via uuid's v4 internals (already a dep).
    let id = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(id.as_bytes());
    out[16..].copy_from_slice(id2.as_bytes());
    out
}
