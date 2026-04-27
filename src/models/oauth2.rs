use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::encryption::EncryptedString;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OAuth2Account {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub provider_user_id: String,
    pub access_token: EncryptedString,
    pub refresh_token: Option<EncryptedString>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}