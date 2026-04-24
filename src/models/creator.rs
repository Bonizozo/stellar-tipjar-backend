use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

lazy_static! {
    /// Alphanumeric + underscores/hyphens only.
    static ref USERNAME_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap();
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Creator {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub password_hash: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(skip_serializing)]
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub backup_code_hashes: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// Request body for creating a new creator
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateCreatorRequest {
    /// Unique username (3–30 chars, alphanumeric/underscore/hyphen)
    #[validate(length(
        min = 3,
        max = 30,
        message = "Username must be between 3 and 30 characters"
    ))]
    #[validate(regex(path = *USERNAME_REGEX, message = "Username may only contain letters, numbers, underscores, and hyphens"))]
    pub username: String,

    /// Stellar wallet address (public key)
    #[validate(custom(function = "crate::validation::stellar::validate_stellar_address"))]
    pub wallet_address: String,
    /// Optional email for tip notifications
    #[validate(email(message = "Invalid email address"))]
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
