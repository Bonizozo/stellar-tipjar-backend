use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tip {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
    pub created_at: DateTime<Utc>,
}

/// Request body for recording a tip
#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordTipRequest {
    /// Username of the creator receiving the tip
    pub username: String,
    /// Amount in XLM (e.g. "10.5")
    pub amount: String,
    /// Stellar transaction hash to verify on-chain
    pub transaction_hash: String,
}

/// Tip record response
#[derive(Debug, Serialize, ToSchema)]
pub struct TipResponse {
    /// Unique identifier
    pub id: Uuid,
    pub creator_username: String,
    /// Amount in XLM
    pub amount: String,
    /// Verified Stellar transaction hash
    pub transaction_hash: String,
    pub created_at: DateTime<Utc>,
}

impl From<Tip> for TipResponse {
    fn from(t: Tip) -> Self {
        Self {
            id: t.id,
            creator_username: t.creator_username,
            amount: t.amount,
            transaction_hash: t.transaction_hash,
            created_at: t.created_at,
        }
    }
}
