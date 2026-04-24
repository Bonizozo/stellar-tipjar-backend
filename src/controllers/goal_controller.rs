use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::goal::{CreateGoalRequest, GoalMilestone, TipGoal};

/// Create a new tip goal for a creator.
pub async fn create_goal(
    pool: &PgPool,
    username: &str,
    req: CreateGoalRequest,
) -> AppResult<TipGoal> {
    // Validate target_amount is a positive number
    let target: f64 = req.target_amount.parse().map_err(|_| {
        AppError::Validation(crate::errors::ValidationError::InvalidRequest {
            message: "target_amount must be a valid number".to_string(),
        })
    })?;
    if target <= 0.0 {
        return Err(AppError::Validation(
            crate::errors::ValidationError::InvalidRequest {
                message: "target_amount must be positive".to_string(),
            },
        ));
    }

    let goal = sqlx::query_as::<_, TipGoal>(
        r#"
        INSERT INTO tip_goals (id, creator_username, title, description, target_amount)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, creator_username, title, description,
                  target_amount::text, current_amount::text, status, created_at, completed_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(username)
    .bind(&req.title)
    .bind(&req.description)
    .bind(&req.target_amount)
    .fetch_one(pool)
    .await?;

    Ok(goal)
}

/// List active goals for a creator.
pub async fn list_goals(pool: &PgPool, username: &str) -> AppResult<Vec<TipGoal>> {
    let goals = sqlx::query_as::<_, TipGoal>(
        r#"
        SELECT id, creator_username, title, description,
               target_amount::text, current_amount::text, status, created_at, completed_at
        FROM tip_goals
        WHERE creator_username = $1 AND status = 'active'
        ORDER BY created_at DESC
        "#,
    )
    .bind(username)
    .fetch_all(pool)
    .await?;

    Ok(goals)
}

/// Get a single goal by id.
pub async fn get_goal(pool: &PgPool, goal_id: Uuid) -> AppResult<TipGoal> {
    sqlx::query_as::<_, TipGoal>(
        r#"
        SELECT id, creator_username, title, description,
               target_amount::text, current_amount::text, status, created_at, completed_at
        FROM tip_goals WHERE id = $1
        "#,
    )
    .bind(goal_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Validation(crate::errors::ValidationError::InvalidRequest {
        message: format!("Goal {} not found", goal_id),
    }))
}

/// Cancel a goal (creator-owned).
pub async fn cancel_goal(pool: &PgPool, goal_id: Uuid, username: &str) -> AppResult<TipGoal> {
    sqlx::query_as::<_, TipGoal>(
        r#"
        UPDATE tip_goals SET status = 'cancelled'
        WHERE id = $1 AND creator_username = $2 AND status = 'active'
        RETURNING id, creator_username, title, description,
                  target_amount::text, current_amount::text, status, created_at, completed_at
        "#,
    )
    .bind(goal_id)
    .bind(username)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Validation(crate::errors::ValidationError::InvalidRequest {
        message: "Goal not found or not cancellable".to_string(),
    }))
}

/// Called after a tip is recorded: update all active goals for the creator and fire milestone notifications.
pub async fn apply_tip_to_goals(pool: &PgPool, username: &str, tip_amount: &str) -> AppResult<()> {
    let amount: f64 = match tip_amount.parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    // Fetch active goals
    let goals = sqlx::query_as::<_, TipGoal>(
        r#"
        SELECT id, creator_username, title, description,
               target_amount::text, current_amount::text, status, created_at, completed_at
        FROM tip_goals WHERE creator_username = $1 AND status = 'active'
        "#,
    )
    .bind(username)
    .fetch_all(pool)
    .await?;

    for goal in goals {
        let prev_current: f64 = goal.current_amount.parse().unwrap_or(0.0);
        let target: f64 = goal.target_amount.parse().unwrap_or(1.0);
        let new_current = prev_current + amount;

        // Update current_amount and mark completed if reached
        let completed = new_current >= target;
        sqlx::query(
            r#"
            UPDATE tip_goals
            SET current_amount = $1,
                status = CASE WHEN $2 THEN 'completed' ELSE status END,
                completed_at = CASE WHEN $2 THEN NOW() ELSE completed_at END
            WHERE id = $3
            "#,
        )
        .bind(new_current)
        .bind(completed)
        .bind(goal.id)
        .execute(pool)
        .await?;

        // Check milestone thresholds: 25%, 50%, 75%, 100%
        let milestones = [25, 50, 75, 100];
        for &pct in &milestones {
            let threshold = target * (pct as f64 / 100.0);
            if prev_current < threshold && new_current >= threshold {
                // Record milestone
                let already_recorded = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM goal_milestones WHERE goal_id = $1 AND threshold_pct = $2)",
                )
                .bind(goal.id)
                .bind(pct)
                .fetch_one(pool)
                .await
                .unwrap_or(false);

                if !already_recorded {
                    sqlx::query(
                        "INSERT INTO goal_milestones (id, goal_id, creator_username, threshold_pct) VALUES ($1, $2, $3, $4)",
                    )
                    .bind(Uuid::new_v4())
                    .bind(goal.id)
                    .bind(username)
                    .bind(pct)
                    .execute(pool)
                    .await?;

                    // Fire notification (non-blocking)
                    let pool2 = pool.clone();
                    let uname = username.to_string();
                    let goal_title = goal.title.clone();
                    let goal_id = goal.id;
                    tokio::spawn(async move {
                        let payload = serde_json::json!({
                            "goal_id": goal_id,
                            "goal_title": goal_title,
                            "threshold_pct": pct,
                        });
                        let _ = crate::controllers::notification_controller::create_notification(
                            &pool2,
                            &uname,
                            "goal_milestone",
                            payload,
                        )
                        .await;
                    });
                }
            }
        }
    }

    Ok(())
}
