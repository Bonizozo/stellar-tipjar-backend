use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreatorVerification {
    pub id: Uuid,
    pub creator_username: String,
    pub status: String,
    pub identity_doc_url: Option<String>,
    pub twitter_handle: Option<String>,
    pub github_handle: Option<String>,
    pub website_url: Option<String>,
    pub rejection_reason: Option<String>,
    pub reviewed_by: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

/// Request to submit a verification application
#[derive(Debug, Deserialize)]
pub struct SubmitVerificationRequest {
    pub identity_doc_url: Option<String>,
    pub twitter_handle: Option<String>,
    pub github_handle: Option<String>,
    pub website_url: Option<String>,
}

/// Admin review decision
#[derive(Debug, Deserialize)]
pub struct ReviewVerificationRequest {
    pub approved: bool,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VerificationResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub status: String,
    pub twitter_handle: Option<String>,
    pub github_handle: Option<String>,
    pub website_url: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

impl From<CreatorVerification> for VerificationResponse {
    fn from(v: CreatorVerification) -> Self {
        Self {
            id: v.id,
            creator_username: v.creator_username,
            status: v.status,
            twitter_handle: v.twitter_handle,
            github_handle: v.github_handle,
            website_url: v.website_url,
            submitted_at: v.submitted_at,
            reviewed_at: v.reviewed_at,
        }
    }
}
