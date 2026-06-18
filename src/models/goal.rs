use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TipGoal {
    pub id: Uuid,
    pub creator_username: String,
    pub title: String,
    pub description: Option<String>,
    pub target_amount: String,
    pub current_amount: String,
    pub status: String,
    pub deadline: Option<DateTime<Utc>>,
    pub is_active: bool,
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

/// Request body for creating a new tip goal.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateGoalRequest {
    /// Short title for the goal (max 100 chars).
    #[validate(length(min = 1, max = 100, message = "Title must be between 1 and 100 characters"))]
    pub title: String,

    /// Optional description of the goal.
    #[validate(length(max = 500, message = "Description must be 500 characters or fewer"))]
    pub description: Option<String>,

    /// Target amount in XLM (e.g. "500.0").
    pub target_amount: String,

    /// Optional deadline for the goal (RFC 3339 timestamp).
    pub deadline: Option<DateTime<Utc>>,
}

/// Goal response including computed progress percentage.
#[derive(Debug, Serialize, ToSchema)]
pub struct TipGoalResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub title: String,
    pub description: Option<String>,
    pub target_amount: String,
    pub current_amount: String,
    /// Completion percentage (0.0 – 100.0).
    pub progress_pct: f64,
    pub status: String,
    pub is_active: bool,
    pub deadline: Option<DateTime<Utc>>,
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
            is_active: g.is_active,
            deadline: g.deadline,
            created_at: g.created_at,
            completed_at: g.completed_at,
        }
    }
}
