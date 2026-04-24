use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TipRefund {
    pub id: Uuid,
    pub tip_id: Uuid,
    pub reason: String,
    pub status: String,
    pub refund_tx_hash: Option<String>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRefundRequest {
    pub tip_id: Uuid,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ReviewRefundRequest {
    /// "approved" or "rejected"
    pub action: String,
    /// Required when action == "approved": the Stellar refund transaction hash
    pub refund_tx_hash: Option<String>,
}
