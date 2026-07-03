use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TipDailyStat {
    pub creator_username: String,
    pub stat_date: NaiveDate,
    pub tip_count: i64,
    pub total_amount: String,
    pub avg_amount: String,
    pub max_amount: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TipSummary {
    pub creator_username: String,
    pub total_tips: i64,
    pub total_amount: String,
    pub avg_amount: String,
    pub max_amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopSupporter {
    pub tipper_wallet: String,
    pub total_amount_xlm: String,
    pub tip_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TipHistoryItem {
    pub id: Uuid,
    pub amount_xlm: String,
    pub transaction_hash: String,
    pub tipper_wallet: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreatorStats {
    pub creator_username: String,
    pub total_amount_xlm: String,
    pub tip_count: i64,
    pub unique_supporters: i64,
    pub top_supporters: Vec<TopSupporter>,
    pub tip_history: Vec<TipHistoryItem>,
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    /// Number of days to look back (default 30, max 365)
    #[serde(default = "StatsQuery::default_days")]
    pub days: i64,
}

impl StatsQuery {
    fn default_days() -> i64 {
        30
    }
    pub fn clamped_days(&self) -> i64 {
        self.days.clamp(1, 365)
    }
}
