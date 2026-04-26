use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "portfolio_media_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Image,
    Video,
    Link,
    Document,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PortfolioItem {
    pub id: Uuid,
    pub creator_username: String,
    pub title: String,
    pub description: Option<String>,
    pub media_type: MediaType,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub display_order: i32,
    pub is_featured: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreatePortfolioItemRequest {
    #[validate(length(min = 1, max = 200, message = "Title must be 1–200 characters"))]
    pub title: String,
    #[validate(length(max = 2000, message = "Description must be 2000 characters or fewer"))]
    pub description: Option<String>,
    pub media_type: Option<MediaType>,
    #[validate(url(message = "url must be a valid URL"))]
    pub url: String,
    #[validate(url(message = "thumbnail_url must be a valid URL"))]
    pub thumbnail_url: Option<String>,
    pub display_order: Option<i32>,
    pub is_featured: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdatePortfolioItemRequest {
    #[validate(length(min = 1, max = 200, message = "Title must be 1–200 characters"))]
    pub title: Option<String>,
    #[validate(length(max = 2000, message = "Description must be 2000 characters or fewer"))]
    pub description: Option<String>,
    pub media_type: Option<MediaType>,
    #[validate(url(message = "url must be a valid URL"))]
    pub url: Option<String>,
    #[validate(url(message = "thumbnail_url must be a valid URL"))]
    pub thumbnail_url: Option<String>,
    pub display_order: Option<i32>,
    pub is_featured: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderRequest {
    /// Ordered list of portfolio item IDs
    pub ids: Vec<Uuid>,
}
