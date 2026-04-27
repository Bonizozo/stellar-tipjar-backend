use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Comment {
    pub id: Uuid,
    pub tip_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub author: String,
    pub body: String,
    pub is_flagged: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateCommentRequest {
    /// Author identifier (username or display name)
    #[validate(length(min = 1, max = 50, message = "Author must be 1–50 characters"))]
    pub author: String,

    /// Comment body (1–1000 characters)
    #[validate(length(min = 1, max = 1000, message = "Body must be 1–1000 characters"))]
    pub body: String,

    /// Optional parent comment ID for nested replies
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct CommentResponse {
    pub id: Uuid,
    pub tip_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub author: String,
    pub body: String,
    pub is_flagged: bool,
    pub created_at: DateTime<Utc>,
}

impl From<Comment> for CommentResponse {
    fn from(c: Comment) -> Self {
        Self {
            id: c.id,
            tip_id: c.tip_id,
            parent_id: c.parent_id,
            author: c.author,
            body: c.body,
            is_flagged: c.is_flagged,
            created_at: c.created_at,
        }
    }
}
