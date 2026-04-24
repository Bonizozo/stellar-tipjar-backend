use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeaderboardSnapshot {
    pub id: Uuid,
    pub period: String,
    pub board_type: String,
    pub rank: i32,
    pub username: String,
    pub score: String,
    pub tip_count: i32,
    pub snapshot_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub rank: i32,
    pub username: String,
    pub score: String,
    pub tip_count: i32,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardResponse {
    pub period: String,
    pub board_type: String,
    pub entries: Vec<LeaderboardEntry>,
    pub snapshot_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct LeaderboardQuery {
    /// Time period: daily, weekly, monthly, all_time (default: all_time)
    #[serde(default = "LeaderboardQuery::default_period")]
    pub period: String,
    /// Max entries to return (default: 10, max: 100)
    #[serde(default = "LeaderboardQuery::default_limit")]
    pub limit: i64,
}

impl LeaderboardQuery {
    fn default_period() -> String {
        "all_time".to_string()
    }
    fn default_limit() -> i64 {
        10
    }
    pub fn validated_period(&self) -> &str {
        match self.period.as_str() {
            "daily" | "weekly" | "monthly" => self.period.as_str(),
            _ => "all_time",
        }
    }
    pub fn validated_limit(&self) -> i64 {
        self.limit.clamp(1, 100)
    }
}
