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

/// Visibility of a tip message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MessageVisibility {
    /// Visible to everyone (default).
    Public,
    /// Visible only to the creator.
    Private,
    /// Hidden by moderation.
    Hidden,
}

impl Default for MessageVisibility {
    fn default() -> Self {
        Self::Public
    }
}

impl MessageVisibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Hidden => "hidden",
        }
    }
}

impl std::fmt::Display for MessageVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─────────────────────────── TipStatus ──────────────────────────────────────

/// Two-phase tip state for on-chain settlement verification.
///
/// State machine: `pending_verification` → `confirmed` | `rejected`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TipStatus {
    PendingVerification,
    Confirmed,
    Rejected,
}

impl TipStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PendingVerification => "pending_verification",
            Self::Confirmed => "confirmed",
            Self::Rejected => "rejected",
        }
    }
}

impl std::fmt::Display for TipStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TipStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending_verification" => Ok(Self::PendingVerification),
            "confirmed" => Ok(Self::Confirmed),
            "rejected" => Ok(Self::Rejected),
            other => Err(format!("unknown tip status: {}", other)),
        }
    }
}

// sqlx encode/decode so the column maps cleanly
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for TipStatus {
    fn decode(
        value: sqlx::postgres::PgValueRef<'r>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let s = <&str as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
        s.parse::<TipStatus>().map_err(|e| e.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for TipStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <&str as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for TipStatus {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <&str as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.as_str(), buf)
    }
}

// ─────────────────────────── Tip entity ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tip {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
    pub message: Option<String>,
    pub message_visibility: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Verification state; defaults to `confirmed` for legacy rows.
    #[sqlx(default)]
    pub status: Option<TipStatus>,
    /// The Stellar source account that submitted the tip transaction.
    #[sqlx(default)]
    pub tipper_source_account: Option<String>,
}

impl Tip {
    /// Return the effective status, treating NULL (legacy rows) as confirmed.
    pub fn effective_status(&self) -> TipStatus {
        self.status.clone().unwrap_or(TipStatus::Confirmed)
    }

    pub fn is_confirmed(&self) -> bool {
        self.effective_status() == TipStatus::Confirmed
    }
}

// ─────────────────────────── Request DTOs ───────────────────────────────────

/// Request body for recording a tip
#[derive(Debug, Clone, Deserialize, Validate, ToSchema)]
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

    /// Optional wallet address for the tipper; used for per-wallet rate limiting.
    #[validate(custom(function = "crate::validation::stellar::validate_stellar_address"))]
    pub tipper_wallet: Option<String>,

    /// Stellar transaction hash — 64 hex characters
    #[validate(length(equal = 64, message = "Transaction hash must be exactly 64 characters"))]
    #[validate(custom(function = "validate_tx_hash"))]
    pub transaction_hash: String,

    /// Optional public message from the tipper (max 280 characters)
    #[validate(length(max = 280, message = "Message must be 280 characters or fewer"))]
    pub message: Option<String>,

    /// Visibility of the message: "public" (default), "private"
    #[serde(default)]
    pub message_visibility: MessageVisibility,

    /// The tipper's Stellar account address (source account of the transaction)
    #[validate(length(min = 56, max = 56, message = "Stellar address must be 56 characters"))]
    pub tipper_source_account: String,

    /// Optional memo included in the Stellar transaction
    pub memo: Option<String>,
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

// ─────────────────────────── Response DTOs ──────────────────────────────────

/// Tip record response – only exposes confirmed tips in the public API.
#[derive(Debug, Serialize, ToSchema)]
pub struct TipResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
    pub message: Option<String>,
    pub message_visibility: String,
    pub created_at: DateTime<Utc>,
    pub status: String,
}

impl From<Tip> for TipResponse {
    fn from(t: Tip) -> Self {
        Self {
            id: t.id,
            creator_username: t.creator_username,
            amount: t.amount,
            transaction_hash: t.transaction_hash,
            message: t.message,
            message_visibility: t.message_visibility.unwrap_or_else(|| "public".to_string()),
            created_at: t.created_at,
            status: t.effective_status().to_string(),
        }
    }
}

/// Request body for reporting a tip message
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ReportMessageRequest {
    /// Reason for the report
    #[validate(length(
        min = 10,
        max = 500,
        message = "Reason must be between 10 and 500 characters"
    ))]
    pub reason: String,
    /// Reporter identifier (username or anonymous)
    #[validate(length(max = 50))]
    pub reported_by: Option<String>,
}
