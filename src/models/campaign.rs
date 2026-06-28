use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Campaign {
    pub id: Uuid,
    pub sponsor_name: String,
    pub creator_username: String,
    pub match_ratio: String,
    pub per_tip_cap: String,
    pub total_budget: String,
    pub remaining_budget: String,
    pub active: bool,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct CampaignContribution {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub tip_id: Uuid,
    pub matched_amount: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct CampaignResponse {
    pub id: Uuid,
    pub sponsor_name: String,
    pub creator_username: String,
    pub match_ratio: String,
    pub per_tip_cap: String,
    pub total_budget: String,
    pub remaining_budget: String,
    pub total_matched_amount: String,
    pub progress_pct: f64,
    pub active: bool,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CampaignMatchResult {
    pub campaign_id: Uuid,
    pub sponsor_name: String,
    pub matched_amount: String,
}
