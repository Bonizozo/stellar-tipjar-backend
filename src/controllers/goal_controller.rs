use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult, ValidationError};
use crate::models::goal::{CreateGoalRequest, GoalMilestone, TipGoal};

// Shared SELECT columns to avoid repetition and keep queries consistent.
const GOAL_COLUMNS: &str = r#"
    id, creator_username, title, description,
    target_amount::text, current_amount::text,
    status, deadline, is_active, created_at, completed_at
"#;

/// Create a new tip goal for a creator.
pub async fn create_goal(
    pool: &PgPool,
    username: &str,
    req: CreateGoalRequest,
) -> AppResult<TipGoal> {
    // Validate target_amount is a positive number.
    let target: f64 = req.target_amount.parse().map_err(|_| {
        AppError::Validation(ValidationError::InvalidRequest {
            message: "target_amount must be a valid number".to_string(),
        })
    })?;
    if target <= 0.0 {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "target_amount must be positive".to_string(),
        }));
    }

    // Ensure the creator exists.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM creators WHERE username = $1)")
        .bind(username)
        .fetch_one(pool)
        .await?;

    if !exists {
        return Err(AppError::CreatorNotFound {
            username: username.to_string(),
        });
    }

    let goal = sqlx::query_as::<_, TipGoal>(&format!(
        r#"
        INSERT INTO tip_goals
            (id, creator_username, title, description, target_amount, deadline, is_active)
        VALUES ($1, $2, $3, $4, $5, $6, true)
        RETURNING {GOAL_COLUMNS}
        "#
    ))
    .bind(Uuid::new_v4())
    .bind(username)
    .bind(&req.title)
    .bind(&req.description)
    .bind(&req.target_amount)
    .bind(req.deadline)
    .fetch_one(pool)
    .await?;

    tracing::info!(
        goal_id = %goal.id,
        creator = %username,
        target = %req.target_amount,
        "Tip goal created"
    );

    Ok(goal)
}

/// List active goals for a creator (includes progress).
pub async fn list_goals(pool: &PgPool, username: &str) -> AppResult<Vec<TipGoal>> {
    // Ensure the creator exists before querying goals.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM creators WHERE username = $1)")
        .bind(username)
        .fetch_one(pool)
        .await?;

    if !exists {
        return Err(AppError::CreatorNotFound {
            username: username.to_string(),
        });
    }

    let goals = sqlx::query_as::<_, TipGoal>(&format!(
        r#"
        SELECT {GOAL_COLUMNS}
        FROM tip_goals
        WHERE creator_username = $1 AND is_active = true
        ORDER BY created_at DESC
        "#
    ))
    .bind(username)
    .fetch_all(pool)
    .await?;

    Ok(goals)
}

/// Get a single goal by id.
pub async fn get_goal(pool: &PgPool, goal_id: Uuid) -> AppResult<TipGoal> {
    sqlx::query_as::<_, TipGoal>(&format!(
        "SELECT {GOAL_COLUMNS} FROM tip_goals WHERE id = $1"
    ))
    .bind(goal_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::Validation(ValidationError::InvalidRequest {
            message: format!("Goal {} not found", goal_id),
        })
    })
}

/// Cancel (soft-delete) a goal — only the owning creator can cancel their own active goal.
pub async fn cancel_goal(pool: &PgPool, goal_id: Uuid, username: &str) -> AppResult<TipGoal> {
    let goal = sqlx::query_as::<_, TipGoal>(&format!(
        r#"
        UPDATE tip_goals
        SET status = 'cancelled', is_active = false
        WHERE id = $1 AND creator_username = $2 AND is_active = true
        RETURNING {GOAL_COLUMNS}
        "#
    ))
    .bind(goal_id)
    .bind(username)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::Validation(ValidationError::InvalidRequest {
            message: "Goal not found or cannot be cancelled (may already be inactive)".to_string(),
        })
    })?;

    tracing::info!(goal_id = %goal_id, creator = %username, "Tip goal cancelled");
    Ok(goal)
}

/// Called after a tip is recorded: update current_amount on all active goals for
/// the creator and fire milestone notifications when a threshold is crossed.
#[tracing::instrument(skip(pool), fields(creator = %username, amount = %tip_amount))]
pub async fn apply_tip_to_goals(pool: &PgPool, username: &str, tip_amount: &str) -> AppResult<()> {
    let amount: f64 = match tip_amount.parse() {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(amount = %tip_amount, "Invalid tip amount for goal update; skipping");
            return Ok(());
        }
    };

    // Fetch all active goals for this creator.
    let goals = sqlx::query_as::<_, TipGoal>(&format!(
        r#"
        SELECT {GOAL_COLUMNS}
        FROM tip_goals
        WHERE creator_username = $1 AND is_active = true
        "#
    ))
    .bind(username)
    .fetch_all(pool)
    .await?;

    for goal in goals {
        let prev_current: f64 = goal.current_amount.parse().unwrap_or(0.0);
        let target: f64 = goal.target_amount.parse().unwrap_or(1.0);
        let new_current = prev_current + amount;
        let completed = new_current >= target;

        // Update current_amount; mark completed + set is_active = false when target reached.
        sqlx::query(
            r#"
            UPDATE tip_goals
            SET
                current_amount = $1,
                status         = CASE WHEN $2 THEN 'completed' ELSE status END,
                is_active      = CASE WHEN $2 THEN false        ELSE is_active END,
                completed_at   = CASE WHEN $2 THEN NOW()        ELSE completed_at END
            WHERE id = $3
            "#,
        )
        .bind(new_current)
        .bind(completed)
        .bind(goal.id)
        .execute(pool)
        .await?;

        if completed {
            tracing::info!(goal_id = %goal.id, creator = %username, "Tip goal completed");
        }

        // Check milestone thresholds: 25%, 50%, 75%, 100%.
        for &pct in &[25i32, 50, 75, 100] {
            let threshold = target * (pct as f64 / 100.0);
            if prev_current < threshold && new_current >= threshold {
                // Only record each milestone once.
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
                        r#"
                        INSERT INTO goal_milestones (id, goal_id, creator_username, threshold_pct)
                        VALUES ($1, $2, $3, $4)
                        "#,
                    )
                    .bind(Uuid::new_v4())
                    .bind(goal.id)
                    .bind(username)
                    .bind(pct)
                    .execute(pool)
                    .await?;

                    tracing::info!(
                        goal_id = %goal.id,
                        creator = %username,
                        pct,
                        "Goal milestone reached"
                    );

                    // Fire milestone notification asynchronously (non-blocking).
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
                        if let Err(e) =
                            crate::controllers::notification_controller::create_notification(
                                &pool2,
                                &uname,
                                "goal_milestone",
                                payload,
                            )
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                goal_id = %goal_id,
                                pct,
                                "Failed to create goal milestone notification"
                            );
                        }
                    });
                }
            }
        }
    }

    Ok(())
}
