use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FeatureFlag {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub rollout_pct: i16,
    pub targeting: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFlagRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rollout_pct: i16,
    #[serde(default = "default_targeting")]
    pub targeting: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFlagRequest {
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub rollout_pct: Option<i16>,
    pub targeting: Option<serde_json::Value>,
}

fn default_targeting() -> serde_json::Value {
    serde_json::json!([])
}
