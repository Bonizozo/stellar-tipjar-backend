use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

lazy_static! {
    static ref USERNAME_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap();
}

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
    #[validate(regex(path = *USERNAME_REGEX, message = "Username may only contain letters, numbers, underscores, and hyphens"))]
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

/// Query filters for listing tips
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct TipFilters {
    /// Filter by minimum amount (inclusive)
    pub min_amount: Option<String>,
    /// Filter by maximum amount (inclusive)
    pub max_amount: Option<String>,
    /// Filter tips created on or after this timestamp (RFC 3339)
    pub from_date: Option<DateTime<Utc>>,
    /// Filter tips created on or before this timestamp (RFC 3339)
    pub to_date: Option<DateTime<Utc>>,
}

/// Sort parameters for listing tips
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TipSortParams {
    /// Field to sort by: `created_at` or `amount` (default: `created_at`)
    #[serde(default = "TipSortParams::default_sort_by")]
    pub sort_by: String,
    /// Sort direction: `asc` or `desc` (default: `desc`)
    #[serde(default = "TipSortParams::default_sort_order")]
    pub sort_order: String,
}

impl Default for TipSortParams {
    fn default() -> Self {
        Self {
            sort_by: Self::default_sort_by(),
            sort_order: Self::default_sort_order(),
        }
    }
}

impl TipSortParams {
    fn default_sort_by() -> String {
        "created_at".to_string()
    }
    fn default_sort_order() -> String {
        "desc".to_string()
    }

    /// Returns a validated (column, direction) pair safe for interpolation.
    pub fn validated(&self) -> (&'static str, &'static str) {
        let col = match self.sort_by.as_str() {
            "amount" => "amount::numeric",
            _ => "created_at",
        };
        let dir = if self.sort_order.eq_ignore_ascii_case("asc") {
            "ASC"
        } else {
            "DESC"
        };
        (col, dir)
    }
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
