use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// Returns the compiled alphanumeric + underscore/hyphen username regex.
///
/// Uses `OnceLock` so the regex is compiled exactly once.
/// The `#[allow]` is justified: the pattern is a compile-time constant
/// string that is syntactically valid — `Regex::new` only returns `Err`
/// for invalid patterns, which can never occur here.
/// Invariant: `Regex::new` on a known-good literal is always `Ok`.
fn username_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    #[allow(clippy::expect_used)]
    RE.get_or_init(|| {
        Regex::new(r"^[a-zA-Z0-9_-]+$")
            .expect("USERNAME_REGEX invariant: regex literal is always valid")
    })
}

// Re-export so that `*USERNAME_REGEX` continues to work in the validator attribute.
lazy_static::lazy_static! {
    pub(crate) static ref USERNAME_REGEX: &'static Regex = username_regex();
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Creator {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Request body for creating a new creator
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateCreatorRequest {
    /// Unique username (3–30 chars, alphanumeric/underscore/hyphen)
    #[validate(length(min = 3, max = 30, message = "Username must be between 3 and 30 characters"))]
    #[validate(regex(path = *USERNAME_REGEX, message = "Username may only contain letters, numbers, underscores, and hyphens"))]
    pub username: String,

    /// Stellar wallet address (public key)
    #[validate(custom(function = "crate::validation::stellar::validate_stellar_address"))]
    pub wallet_address: String,
    /// Optional email for tip notifications
    pub email: Option<String>,
}

/// Creator profile response
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatorResponse {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<Creator> for CreatorResponse {
    fn from(c: Creator) -> Self {
        Self {
            id: c.id,
            username: c.username,
            wallet_address: c.wallet_address,
            email: c.email,
            created_at: c.created_at,
        }
    }
}
