use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TipGoal {
    pub id: Uuid,
    pub creator_username: String,
    pub title: String,
    pub description: Option<String>,
    pub target_amount: String,
    pub current_amount: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GoalMilestone {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub creator_username: String,
    pub threshold_pct: i32,
    pub reached_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateGoalRequest {
    pub title: String,
    pub description: Option<String>,
    pub target_amount: String,
}

#[derive(Debug, Serialize)]
pub struct TipGoalResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub title: String,
    pub description: Option<String>,
    pub target_amount: String,
    pub current_amount: String,
    pub progress_pct: f64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<TipGoal> for TipGoalResponse {
    fn from(g: TipGoal) -> Self {
        let target: f64 = g.target_amount.parse().unwrap_or(1.0);
        let current: f64 = g.current_amount.parse().unwrap_or(0.0);
        let progress_pct = if target > 0.0 {
            (current / target * 100.0).min(100.0)
        } else {
            0.0
        };
        Self {
            id: g.id,
            creator_username: g.creator_username,
            title: g.title,
            description: g.description,
            target_amount: g.target_amount,
            current_amount: g.current_amount,
            progress_pct,
            status: g.status,
            created_at: g.created_at,
            completed_at: g.completed_at,
        }
    }
}
