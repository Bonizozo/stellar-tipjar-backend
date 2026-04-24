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
    pub permissions: Vec<String>,
    pub active: bool,
    pub usage_count: i64,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Returned only on creation/rotation — the secret is never retrievable again.
#[derive(Debug, Serialize)]
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub key: String,
    pub secret: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// Safe view (no secret) for list/get responses.
#[derive(Debug, Serialize)]
pub struct ApiKeyView {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub active: bool,
    pub usage_count: i64,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<ApiKey> for ApiKeyView {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id,
            key: k.key,
            name: k.name,
            permissions: k.permissions,
            active: k.active,
            usage_count: k.usage_count,
            created_at: k.created_at,
            rotated_at: k.rotated_at,
            revoked_at: k.revoked_at,
        }
    }
}

impl ApiKey {
    pub async fn create(
        pool: &PgPool,
        name: &str,
        permissions: &[String],
    ) -> Result<ApiKeyCreated, sqlx::Error> {
        let key = generate_key("tjk");
        let secret = generate_key("tjs");

        let row: ApiKey = sqlx::query_as(
            "INSERT INTO api_keys (key, secret, name, permissions)
             VALUES ($1, $2, $3, $4)
             RETURNING id, key, secret, name, permissions, active, usage_count,
                       created_at, rotated_at, revoked_at",
        )
        .bind(&key)
        .bind(&secret)
        .bind(name)
        .bind(permissions)
        .fetch_one(pool)
        .await?;

        Ok(ApiKeyCreated {
            id: row.id,
            key: row.key,
            secret,
            name: row.name,
            permissions: row.permissions,
            created_at: row.created_at,
        })
    }

    pub async fn list(pool: &PgPool) -> Result<Vec<ApiKey>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, key, secret, name, permissions, active, usage_count,
                    created_at, rotated_at, revoked_at
             FROM api_keys ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn get_by_key(pool: &PgPool, key: &str) -> Result<ApiKey, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, key, secret, name, permissions, active, usage_count,
                    created_at, rotated_at, revoked_at
             FROM api_keys WHERE key = $1",
        )
        .bind(key)
        .fetch_one(pool)
        .await
    }

    /// Verify key+secret and return the key row if valid and not revoked.
    pub async fn verify(pool: &PgPool, key: &str, secret: &str) -> Result<ApiKey, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, key, secret, name, permissions, active, usage_count,
                    created_at, rotated_at, revoked_at
             FROM api_keys
             WHERE key = $1 AND secret = $2 AND active = true AND revoked_at IS NULL",
        )
        .bind(key)
        .bind(secret)
        .fetch_one(pool)
        .await
    }

    /// Increment usage counter (fire-and-forget; ignore errors).
    pub async fn record_usage(pool: &PgPool, key: &str) {
        let _ = sqlx::query("UPDATE api_keys SET usage_count = usage_count + 1 WHERE key = $1")
            .bind(key)
            .execute(pool)
            .await;
    }

    /// Rotate: deactivate old key, create a new one with the same name + permissions.
    pub async fn rotate(pool: &PgPool, key: &str) -> Result<ApiKeyCreated, sqlx::Error> {
        let row: ApiKey = sqlx::query_as(
            "UPDATE api_keys SET active = false, rotated_at = NOW()
             WHERE key = $1 AND active = true AND revoked_at IS NULL
             RETURNING id, key, secret, name, permissions, active, usage_count,
                       created_at, rotated_at, revoked_at",
        )
        .bind(key)
        .fetch_one(pool)
        .await?;

        Self::create(pool, &row.name, &row.permissions).await
    }

    /// Revoke: permanently disable the key.
    pub async fn revoke(pool: &PgPool, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE api_keys SET active = false, revoked_at = NOW()
             WHERE key = $1 AND revoked_at IS NULL",
        )
        .bind(key)
        .execute(pool)
        .await?;
        Ok(())
    }
}

/// Generates a prefixed, cryptographically random hex key.
fn generate_key(prefix: &str) -> String {
    let id1 = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(id1.as_bytes());
    bytes[16..].copy_from_slice(id2.as_bytes());
    format!("{}_{}", prefix, hex::encode(bytes))
}
