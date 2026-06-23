use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::crypto::encryption::EncryptedString;

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
    pub password_hash: EncryptedString,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(skip_serializing)]
    pub totp_secret: Option<EncryptedString>,
    pub totp_enabled: bool,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub backup_code_hashes: Vec<EncryptedString>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    #[sqlx(default)]
    pub bio: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub is_verified: bool,
    #[serde(default)]
    #[sqlx(default)]
    pub social_links: serde_json::Value,
    #[serde(default)]
    #[sqlx(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub tags: Vec<String>,
}

/// A social link entry stored inside `Creator.social_links` (JSONB array).
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct SocialLink {
    /// Platform name (e.g. "twitter", "github", "website")
    #[validate(length(min = 1, max = 50, message = "Platform must be 1–50 chars"))]
    pub platform: String,
    /// URL or handle
    #[validate(length(min = 1, max = 500, message = "URL must be 1–500 chars"))]
    pub url: String,
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

    /// Optional biography (max 1000 chars)
    #[validate(length(max = 1000, message = "Bio must be at most 1000 characters"))]
    pub bio: Option<String>,

    /// Optional display name (max 100 chars)
    #[validate(length(max = 100, message = "Display name must be at most 100 characters"))]
    pub display_name: Option<String>,

    /// Optional avatar URL (max 500 chars)
    #[validate(length(max = 500, message = "Avatar URL must be at most 500 characters"))]
    pub avatar_url: Option<String>,

    /// Optional list of social links
    pub social_links: Option<Vec<SocialLink>>,

    /// Optional categories
    pub categories: Option<Vec<String>>,

    /// Optional tags
    pub tags: Option<Vec<String>>,
}

/// Request body used to update a creator's Stellar wallet address.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateCreatorWalletRequest {
    /// New Stellar wallet address (public key) to associate with the creator.
    #[validate(custom(function = "crate::validation::stellar::validate_stellar_address"))]
    pub new_wallet_address: String,

    /// Base64-encoded ed25519 signature proving ownership of the new wallet.
    /// The signature must verify against the new wallet address string.
    #[validate(length(min = 1, message = "Signature must be provided"))]
    pub signature: String,
}

/// Creator profile response
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatorResponse {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub bio: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_verified: bool,
    pub social_links: serde_json::Value,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
}

impl From<Creator> for CreatorResponse {
    fn from(c: Creator) -> Self {
        Self {
            id: c.id,
            username: c.username,
            wallet_address: c.wallet_address,
            email: c.email,
            created_at: c.created_at,
            bio: c.bio,
            display_name: c.display_name,
            avatar_url: c.avatar_url,
            is_verified: c.is_verified,
            social_links: c.social_links,
            categories: c.categories,
            tags: c.tags,
        }
    }
}

/// Request body for updating creator profile fields
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateCreatorProfileRequest {
    /// Optional biography (max 1000 chars)
    #[validate(length(max = 1000, message = "Bio must be at most 1000 characters"))]
    pub bio: Option<String>,

    /// Optional display name (max 100 chars)
    #[validate(length(max = 100, message = "Display name must be at most 100 characters"))]
    pub display_name: Option<String>,

    /// Optional avatar URL (max 500 chars)
    #[validate(length(max = 500, message = "Avatar URL must be at most 500 characters"))]
    pub avatar_url: Option<String>,

    /// Optional list of social links
    #[validate]
    pub social_links: Option<Vec<SocialLink>>,

    /// Optional categories
    pub categories: Option<Vec<String>>,

    /// Optional tags
    pub tags: Option<Vec<String>>,
}
