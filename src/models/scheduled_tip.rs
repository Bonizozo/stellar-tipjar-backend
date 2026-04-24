use chrono::{DateTime, Datelike, Duration, Months, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScheduledTip {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub tipper_ref: String,
    pub message: Option<String>,
    pub status: String,
    pub scheduled_at: DateTime<Utc>,
    pub is_recurring: bool,
    pub recurrence_rule: Option<String>,
    pub recurrence_end: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub run_count: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateScheduledTipRequest {
    pub creator_username: String,
    pub amount: String,
    pub tipper_ref: String,
    pub message: Option<String>,
    /// When to send the tip (one-shot or first occurrence for recurring)
    pub scheduled_at: DateTime<Utc>,
    /// If true, `recurrence_rule` is required
    #[serde(default)]
    pub is_recurring: bool,
    /// "daily" | "weekly" | "monthly"
    pub recurrence_rule: Option<String>,
    /// Stop recurring after this date (None = indefinite)
    pub recurrence_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateScheduledTipRequest {
    pub scheduled_at: Option<DateTime<Utc>>,
    pub amount: Option<String>,
    pub message: Option<String>,
    pub recurrence_rule: Option<String>,
    pub recurrence_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ScheduledTipResponse {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub tipper_ref: String,
    pub message: Option<String>,
    pub status: String,
    pub scheduled_at: DateTime<Utc>,
    pub is_recurring: bool,
    pub recurrence_rule: Option<String>,
    pub recurrence_end: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub run_count: i32,
    pub created_at: DateTime<Utc>,
}

impl From<ScheduledTip> for ScheduledTipResponse {
    fn from(s: ScheduledTip) -> Self {
        Self {
            id: s.id,
            creator_username: s.creator_username,
            amount: s.amount,
            tipper_ref: s.tipper_ref,
            message: s.message,
            status: s.status,
            scheduled_at: s.scheduled_at,
            is_recurring: s.is_recurring,
            recurrence_rule: s.recurrence_rule,
            recurrence_end: s.recurrence_end,
            next_run_at: s.next_run_at,
            last_run_at: s.last_run_at,
            run_count: s.run_count,
            created_at: s.created_at,
        }
    }
}

/// Compute the next run time from `from` given a recurrence rule.
/// Returns `None` when the rule is unrecognised or the next run would exceed `end`.
pub fn next_run(
    from: DateTime<Utc>,
    rule: &str,
    end: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let next = match rule {
        "daily" => from + Duration::days(1),
        "weekly" => from + Duration::weeks(1),
        "monthly" => {
            // Add one calendar month, clamping to the last day of the target month.
            from.checked_add_months(Months::new(1))?
        }
        _ => return None,
    };

    if let Some(end) = end {
        if next > end {
            return None;
        }
    }
    Some(next)
}
