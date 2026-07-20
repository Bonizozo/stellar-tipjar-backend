use std::time::Instant;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::errors::{AppError, AppResult};
use crate::models::pagination::{PaginatedResponse, PaginationParams};
use crate::models::tip::{RecordTipRequest, Tip};
use crate::cache::{redis_client, keys};
use crate::metrics::collectors::DB_QUERY_DURATION_SECONDS; // Kept from your branch
use crate::db::transaction;

#[tracing::instrument(skip(state), fields(username = %req.username, amount = %req.amount))]
pub async fn record_tip(state: &AppState, req: RecordTipRequest) -> AppResult<Tip> {
    let mut tx = transaction::begin_transaction(&state.db)
        .await
        .map_err(AppError::from)?;

    let start = Instant::now();
    // Pass state into the internal helper to support WebSocket broadcasting
    let tip = record_tip_in_tx(state, &mut tx, &req).await?;
    tx.commit().await?;
    let duration = start.elapsed();

    // Record your Prometheus metric
    DB_QUERY_DURATION_SECONDS.observe(duration.as_secs_f64());

    QueryLogger::log_query("INSERT tips + tip_logs (transaction)", duration);
    state.performance.track_query("tip_atomic_record", duration);

    // Cache invalidation (using our state.redis fix)
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let tips_key = keys::creator_tips(&tip.creator_username);
        let _ = redis_client::del(&mut conn, &[tips_key.as_str()]).await;
    }

    // Notify external services via webhook.
    let payload = serde_json::to_value(&tip).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize tip webhook payload");
        crate::errors::AppError::internal()
    })?;
    crate::webhooks::trigger_webhooks(state.db.clone(), "tip.recorded", payload).await;

    Ok(tip)
}

/// Lower-level tip recording that executes within an existing transaction.
pub async fn record_tip_in_tx(
    state: &AppState, // Added state parameter to fix scope issue in Main
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &RecordTipRequest,
) -> AppResult<Tip> {
    let query_tip = r#"
        INSERT INTO tips (id, creator_username, amount, transaction_hash, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        RETURNING id, creator_username, amount, transaction_hash, created_at
        "#;

    let tip = sqlx::query_as::<_, Tip>(query_tip)
        .bind(Uuid::new_v4())
        .bind(&req.username)
        .bind(&req.amount)
        .bind(&req.transaction_hash)
        .fetch_one(&mut **tx)
        .await?;

    // Log the action in the database
    let query_log = r#"
        INSERT INTO tip_logs (tip_id, creator_username, action)
        VALUES ($1, $2, 'recorded_atomic')
        "#;

    sqlx::query(query_log)
        .bind(&tip.id)
        .bind(&tip.creator_username)
        .execute(&mut **tx)
        .await?;

    // Broadcast to WebSocket (Main branch feature).
    // Amount is stored as a decimal string (e.g. "10.5 XLM"); we convert to
    // stroops for the wire event, falling back to 0 on any parse error so the
    // broadcast never blocks the happy path.
    let amount_stroops: u64 = tip
        .amount
        .parse::<f64>()
        .map(|xlm| (xlm * 10_000_000.0) as u64)
        .unwrap_or(0);
    let event = crate::ws::TipEvent {
        creator_id: tip.creator_username.clone(),
        tipper_id: req.transaction_hash.clone(),
        amount: amount_stroops,
        timestamp: tip.created_at.timestamp(),
    };
    crate::ws::broadcast_tip(&state.broadcast_tx, event).await;

    Ok(tip)
}

/// Fetch all tips for a creator without pagination (kept for internal use).
pub async fn get_tips_for_creator(state: &AppState, username: &str) -> AppResult<Vec<Tip>> {
    let query = r#"
        SELECT id, creator_username, amount, transaction_hash, created_at
        FROM tips
        WHERE creator_username = $1
        ORDER BY created_at DESC
        "#;

    let start = Instant::now();
    let tips = sqlx::query_as::<_, Tip>(query)
        .bind(username)
        .fetch_all(&state.db)
        .await?;
    let duration = start.elapsed();

    // Record your Prometheus metric
    DB_QUERY_DURATION_SECONDS.observe(duration.as_secs_f64());

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);

    // Populate cache
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let cache_key = keys::creator_tips(username);
        let _ = redis_client::set(&mut conn, &cache_key, &tips, redis_client::TTL_TIPS).await;
    }

    Ok(tips)
}

pub async fn get_tips_paginated(
    state: &AppState,
    username: &str,
    params: PaginationParams,
) -> AppResult<PaginatedResponse<Tip>> {
    let params = params.validated();

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tips WHERE creator_username = $1",
    )
    .bind(username)
    .fetch_one(&state.db)
    .await?;

    let start = Instant::now();
    let tips = sqlx::query_as::<_, Tip>(
        r#"
        SELECT id, creator_username, amount, transaction_hash, created_at
        FROM tips
        WHERE creator_username = $1
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(username)
    .bind(params.limit)
    .bind(params.offset())
    .fetch_all(&state.db)
    .await?;
    let duration = start.elapsed();

    // Record your Prometheus metric for paginated queries too
    DB_QUERY_DURATION_SECONDS.observe(duration.as_secs_f64());

    Ok(PaginatedResponse::new(tips, total, &params))
}