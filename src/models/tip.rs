use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tip {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Request body for recording a tip
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RecordTipRequest {
    /// Username of the creator receiving the tip
    #[validate(length(
        min = 3,
        max = 30,
        message = "Username must be between 3 and 30 characters"
    ))]
    pub username: String,

    /// Amount in XLM (e.g. "10.5"), positive, max 7 decimal places
    #[validate(custom(function = "crate::validation::amount::validate_xlm_amount"))]
    pub amount: String,

    /// Stellar transaction hash — 64 hex characters
    #[validate(length(equal = 64, message = "Transaction hash must be exactly 64 characters"))]
    #[validate(custom(function = "validate_tx_hash"))]
    pub transaction_hash: String,

    /// Optional public message from the tipper (max 280 characters)
    #[validate(length(max = 280, message = "Message must be 280 characters or fewer"))]
    pub message: Option<String>,
}

fn validate_tx_hash(hash: &str) -> Result<(), validator::ValidationError> {
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut e = validator::ValidationError::new("invalid_tx_hash");
        e.message = Some("Transaction hash must contain only hexadecimal characters".into());
        return Err(e);
    }
    Ok(())
}

/// Tip record response
#[derive(Debug, Serialize, ToSchema)]
pub struct TipResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<Tip> for TipResponse {
    fn from(t: Tip) -> Self {
        Self {
            id: t.id,
            creator_username: t.creator_username,
            amount: t.amount,
            transaction_hash: t.transaction_hash,
            message: t.message,
            created_at: t.created_at,
        }
    }
}
