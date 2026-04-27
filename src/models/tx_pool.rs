use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "tx_pool_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TxPoolStatus {
    Pending,
    Processing,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TxPool {
    pub id: Uuid,
    pub transaction_hash: String,
    pub status: TxPoolStatus,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub next_retry_at: DateTime<Utc>,
}

/// Enqueue a new transaction.
#[derive(Debug, Deserialize)]
pub struct EnqueueTxRequest {
    pub transaction_hash: String,
    /// Optional caller-supplied metadata (e.g. tip_id, creator).
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Override default max retries (1–10).
    pub max_retries: Option<i32>,
}

/// Public status response.
#[derive(Debug, Serialize)]
pub struct TxPoolStatusResponse {
    pub id: Uuid,
    pub transaction_hash: String,
    pub status: TxPoolStatus,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<TxPool> for TxPoolStatusResponse {
    fn from(t: TxPool) -> Self {
        Self {
            id: t.id,
            transaction_hash: t.transaction_hash,
            status: t.status,
            retry_count: t.retry_count,
            max_retries: t.max_retries,
            last_error: t.last_error,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

/// Monitoring snapshot.
#[derive(Debug, Serialize)]
pub struct TxPoolStats {
    pub pending: i64,
    pub processing: i64,
    pub confirmed: i64,
    pub failed: i64,
}
