use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::scheduled_tip::{
    CreateScheduledTipRequest, ScheduledTip, UpdateScheduledTipRequest,
};

pub async fn create(pool: &PgPool, req: CreateScheduledTipRequest) -> Result<ScheduledTip, sqlx::Error> {
    let next_run_at = if req.is_recurring {
        Some(req.scheduled_at)
    } else {
        Some(req.scheduled_at)
    };

    sqlx::query_as::<_, ScheduledTip>(
        "INSERT INTO scheduled_tips
            (creator_username, amount, tipper_ref, message, scheduled_at,
             is_recurring, recurrence_rule, recurrence_end, next_run_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(&req.creator_username)
    .bind(&req.amount)
    .bind(&req.tipper_ref)
    .bind(&req.message)
    .bind(req.scheduled_at)
    .bind(req.is_recurring)
    .bind(&req.recurrence_rule)
    .bind(req.recurrence_end)
    .bind(next_run_at)
    .fetch_one(pool)
    .await
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<ScheduledTip, sqlx::Error> {
    sqlx::query_as::<_, ScheduledTip>("SELECT * FROM scheduled_tips WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn list_for_tipper(
    pool: &PgPool,
    tipper_ref: &str,
) -> Result<Vec<ScheduledTip>, sqlx::Error> {
    sqlx::query_as::<_, ScheduledTip>(
        "SELECT * FROM scheduled_tips WHERE tipper_ref = $1 ORDER BY scheduled_at DESC",
    )
    .bind(tipper_ref)
    .fetch_all(pool)
    .await
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    req: UpdateScheduledTipRequest,
) -> Result<ScheduledTip, sqlx::Error> {
    sqlx::query_as::<_, ScheduledTip>(
        "UPDATE scheduled_tips SET
            scheduled_at     = COALESCE($2, scheduled_at),
            next_run_at      = COALESCE($2, next_run_at),
            amount           = COALESCE($3, amount),
            message          = COALESCE($4, message),
            recurrence_rule  = COALESCE($5, recurrence_rule),
            recurrence_end   = COALESCE($6, recurrence_end),
            updated_at       = NOW()
         WHERE id = $1 AND status = 'pending'
         RETURNING *",
    )
    .bind(id)
    .bind(req.scheduled_at)
    .bind(req.amount)
    .bind(req.message)
    .bind(req.recurrence_rule)
    .bind(req.recurrence_end)
    .fetch_one(pool)
    .await
}

pub async fn cancel(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE scheduled_tips SET status = 'cancelled', updated_at = NOW()
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// Fetch all pending scheduled tips whose next_run_at is due.
pub async fn due_tips(pool: &PgPool) -> Result<Vec<ScheduledTip>, sqlx::Error> {
    sqlx::query_as::<_, ScheduledTip>(
        "SELECT * FROM scheduled_tips
         WHERE status = 'pending' AND next_run_at <= $1
         ORDER BY next_run_at ASC
         LIMIT 100",
    )
    .bind(Utc::now())
    .fetch_all(pool)
    .await
}

/// Mark a scheduled tip as processing (optimistic lock via status check).
pub async fn mark_processing(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE scheduled_tips SET status = 'processing', updated_at = NOW()
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// After successful execution: advance next_run_at or mark completed.
pub async fn mark_completed(
    pool: &PgPool,
    id: Uuid,
    next_run_at: Option<chrono::DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    let new_status = if next_run_at.is_some() { "pending" } else { "completed" };
    sqlx::query(
        "UPDATE scheduled_tips SET
            status       = $2,
            last_run_at  = NOW(),
            next_run_at  = $3,
            run_count    = run_count + 1,
            last_error   = NULL,
            updated_at   = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .bind(new_status)
    .bind(next_run_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// After a failed execution: record error and revert to pending for retry.
pub async fn mark_failed(pool: &PgPool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE scheduled_tips SET
            status     = 'failed',
            last_error = $2,
            updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}
