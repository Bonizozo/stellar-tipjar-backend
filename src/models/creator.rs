use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Creator {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Request body for creating a new creator
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCreatorRequest {
    /// Unique username for the creator
    pub username: String,
    /// Stellar wallet address (public key)
    pub wallet_address: String,
    /// Optional email for tip notifications
    pub email: Option<String>,
}

/// Creator profile response
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatorResponse {
    /// Unique identifier
    pub id: Uuid,
    pub username: String,
    /// Stellar wallet address (public key)
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
