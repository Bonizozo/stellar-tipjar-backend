use std::{net::IpAddr, time::Instant};
use uuid::Uuid;

use crate::cache::{keys, redis_client};
use crate::controllers::{campaign_controller, team_controller};
use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::db::transaction;
use crate::errors::{AppError, AppResult, DatabaseError};
use crate::metrics::collectors::{
    DB_QUERY_DURATION_SECONDS, TIPS_AMOUNT_XLM, TIPS_CREATED_TOTAL, TIPS_FAILED_TOTAL,
};
use crate::models::pagination::{
    CursorDirection, KeysetCursor, PaginatedResponse, PaginationParams,
};
use crate::models::tip::{
    RecordTipRequest, ReportMessageRequest, Tip, TipFilters, TipSortParams, TipStatus,
};
use crate::moderation::ContentType;
use crate::queue::VerificationJob;
use crate::validation::amount::xlm_to_stroops_str;

#[derive(Debug, Clone, Copy, Default)]
pub struct TipRecordContext {
    pub ip: Option<IpAddr>,
}

// ─────────────────────────── record_tip ─────────────────────────────────────

/// Insert the tip as `pending_verification` and enqueue an async verification job.
///
/// Uses `ON CONFLICT DO NOTHING` so duplicate tx_hash submissions from concurrent
/// clients simply return a `Conflict` error rather than crashing.
///
/// No webhooks fire here – they are deferred until `confirm_tip`.
#[tracing::instrument(skip(state), fields(username = %req.username, amount = %req.amount, tx_hash = %req.transaction_hash))]
pub async fn record_tip(state: &AppState, req: RecordTipRequest) -> AppResult<Tip> {
    record_tip_with_context(state, req, TipRecordContext::default()).await
}

#[tracing::instrument(skip(state), fields(username = %req.username, amount = %req.amount, ip = ?context.ip))]
pub async fn record_tip_with_context(
    state: &AppState,
    req: RecordTipRequest,
    context: TipRecordContext,
) -> AppResult<Tip> {
    // Moderate the tip message when provided.
    if let Some(ref msg) = req.message {
        if !msg.trim().is_empty() {
            let moderation = state
                .moderation
                .check_content(msg, ContentType::TipMessage, None)
                .await;
            if moderation.has_high_confidence_violation(0.90) {
                TIPS_FAILED_TOTAL
                    .with_label_values(&["moderation_rejected"])
                    .inc();
                return Err(AppError::Validation(
                    crate::errors::ValidationError::InvalidRequest {
                        message: "Tip message was rejected by content moderation".to_string(),
                    },
                ));
            }
        }
    }

    let start = Instant::now();
    let tip_id = Uuid::new_v4();

    let tip = sqlx::query_as::<_, Tip>(
        r#"
        INSERT INTO tips
            (id, creator_username, amount, transaction_hash, tipper_source_account, status, created_at)
        VALUES
            ($1, $2, $3, $4, $5, 'pending_verification', NOW())
        ON CONFLICT (transaction_hash) DO NOTHING
        RETURNING id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account
        "#,
    )
    .bind(tip_id)
    .bind(&req.username)
    .bind(&req.amount)
    .bind(&req.transaction_hash)
    .bind(&req.tipper_source_account)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict {
        code: "DUPLICATE_TX_HASH",
        message: format!(
            "A tip with transaction hash '{}' has already been submitted",
            req.transaction_hash
        ),
    })?;

    let duration = start.elapsed();
    DB_QUERY_DURATION_SECONDS
        .with_label_values(&["tip_record_pending"])
        .observe(duration.as_secs_f64());
    QueryLogger::log_query("INSERT tips (pending_verification)", duration);
    state
        .performance
        .track_query("tip_record_pending", duration);

    // Log the initial insert in the audit table
    let _ = sqlx::query(
        "INSERT INTO tip_logs (tip_id, creator_username, action) VALUES ($1, $2, 'submitted')",
    )
    .bind(&tip.id)
    .bind(&tip.creator_username)
    .execute(&state.db)
    .await;

    // Parse amount to stroops for the verification job
    let amount_stroops = xlm_to_stroops_str(&req.amount).map_err(|e| {
        AppError::Validation(crate::errors::ValidationError::InvalidRequest {
            message: format!("Invalid tip amount: {}", e),
        })
    })?;

    // Fetch creator wallet address for destination verification
    let destination: String =
        sqlx::query_scalar("SELECT wallet_address FROM creators WHERE username = $1")
            .bind(&req.username)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::CreatorNotFound {
                username: req.username.clone(),
            })?;

    // Enqueue verification job; failure here is best-effort – reconciliation will retry
    let job = VerificationJob {
        tip_id: tip.id,
        transaction_hash: req.transaction_hash.clone(),
        amount_stroops,
        destination,
        expected_memo: req.memo.clone(),
        source_account: req.tipper_source_account.clone(),
        attempt: 0,
    };

    if let Err(e) = state.queue.enqueue(job).await {
        tracing::error!(
            tip_id = %tip.id,
            error = %e,
            "Failed to enqueue verification job; reconciliation will retry"
        );
    }

    // Broadcast real-time event (tip is pending – UI may show a spinner)
    let event = crate::ws::TipEvent {
        creator_id: tip.creator_username.clone(),
        tipper_id: req.tipper_source_account.clone(),
        amount: tip.amount.parse::<u64>().unwrap_or(0),
        timestamp: tip.created_at.timestamp(),
    };
    crate::ws::broadcast_tip(&state.broadcast_tx, event).await;

    // Cache invalidation
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let tips_key = keys::creator_tips_pattern(&tip.creator_username);
        let _ = redis_client::del(&mut conn, &[tips_key.as_str()]).await;
    }

    TIPS_CREATED_TOTAL.inc();
    if let Ok(amount) = tip.amount.parse::<f64>() {
        TIPS_AMOUNT_XLM.observe(amount);
    }

    Ok(tip)
}

// ─────────────────────────── confirm_tip ────────────────────────────────────

/// Transition a tip from `pending_verification` to `confirmed`.
///
/// Only fires webhooks and updates analytics **after** confirmation.
pub async fn confirm_tip(state: &AppState, tip_id: Uuid) -> AppResult<()> {
    let tip = sqlx::query_as::<_, Tip>(
        r#"
        UPDATE tips
        SET    status = 'confirmed'
        WHERE  id     = $1
          AND  status = 'pending_verification'
        RETURNING id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account
        "#,
    )
    .bind(tip_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Database(DatabaseError::NotFound {
        entity: "tip",
        identifier: tip_id.to_string(),
    }))?;

    // Audit log
    let _ = sqlx::query(
        "INSERT INTO tip_logs (tip_id, creator_username, action) VALUES ($1, $2, 'confirmed')",
    )
    .bind(tip.id)
    .bind(&tip.creator_username)
    .execute(&state.db)
    .await;

    // Cache invalidation
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let _ = redis_client::del(
            &mut conn,
            &[keys::creator_tips_pattern(&tip.creator_username).as_str()],
        )
        .await;
    }

    // Webhooks fire ONLY for confirmed tips
    let payload = serde_json::to_value(&tip).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize tip webhook payload");
        AppError::internal()
    })?;
    crate::webhooks::trigger_webhooks(state.db.clone(), "tip.confirmed", payload).await;

    tracing::info!(tip_id = %tip_id, "Tip confirmed and webhooks triggered");
    Ok(())
}

// ─────────────────────────── reject_tip ─────────────────────────────────────

/// Transition a tip from `pending_verification` to `rejected` with a reason.
///
/// Rejected tips do NOT fire webhooks or update leaderboards.
pub async fn reject_tip(state: &AppState, tip_id: Uuid, reason: &str) -> AppResult<()> {
    let tip = sqlx::query_as::<_, Tip>(
        r#"
        UPDATE tips
        SET    status = 'rejected'
        WHERE  id     = $1
          AND  status = 'pending_verification'
        RETURNING id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account
        "#,
    )
    .bind(tip_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Database(DatabaseError::NotFound {
        entity: "tip",
        identifier: tip_id.to_string(),
    }))?;

    // Audit log with rejection reason
    let action = format!("rejected: {}", reason);
    let _ =
        sqlx::query("INSERT INTO tip_logs (tip_id, creator_username, action) VALUES ($1, $2, $3)")
            .bind(tip.id)
            .bind(&tip.creator_username)
            .bind(&action)
            .execute(&state.db)
            .await;

    // Cache invalidation
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let _ = redis_client::del(
            &mut conn,
            &[keys::creator_tips_pattern(&tip.creator_username).as_str()],
        )
        .await;
    }

    tracing::info!(tip_id = %tip_id, reason = %reason, "Tip rejected");
    Ok(())
}

// ─────────────────────────── read helpers ───────────────────────────────────

/// Fetch all **confirmed** tips for a creator (for external API responses).
pub async fn get_tips_for_creator(state: &AppState, username: &str) -> AppResult<Vec<Tip>> {
    let query = r#"
        SELECT id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account
        FROM tips
        WHERE creator_username = $1
          AND status = 'confirmed'
        ORDER BY created_at DESC
        "#;

    let start = Instant::now();
    let tips = sqlx::query_as::<_, Tip>(query)
        .bind(username)
        .fetch_all(&state.db)
        .await?;
    let duration = start.elapsed();

    DB_QUERY_DURATION_SECONDS
        .with_label_values(&["tips_list_by_creator"])
        .observe(duration.as_secs_f64());
    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);

    // Populate cache
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let cache_key = format!("creator:{}:tips:all", username);
        let _ = redis_client::set(&mut conn, &cache_key, &tips, redis_client::TTL_TIPS).await;
    }

    Ok(tips)
}

pub async fn get_tips_paginated(
    state: &AppState,
    username: Option<&str>,
    params: PaginationParams,
    filters: TipFilters,
    _sort: TipSortParams,
) -> AppResult<PaginatedResponse<Tip>> {
    let params = params.validated();
    let min_amount = filters.min_amount.as_deref();
    let max_amount = filters.max_amount.as_deref();
    let from_date = filters.from_date;
    let to_date = filters.to_date;

    let mut conditions: Vec<String> = Vec::new();
    let mut bind_idx: i32 = 1;

    if username.is_some() {
        conditions.push(format!("creator_username = ${bind_idx}"));
        bind_idx += 1;
    }
    if min_amount.is_some() {
        conditions.push(format!("amount::numeric >= ${}::numeric", bind_idx));
        bind_idx += 1;
    }
    if max_amount.is_some() {
        conditions.push(format!("amount::numeric <= ${}::numeric", bind_idx));
        bind_idx += 1;
    }
    if from_date.is_some() {
        conditions.push(format!("created_at >= ${bind_idx}"));
        bind_idx += 1;
    }
    if to_date.is_some() {
        conditions.push(format!("created_at <= ${bind_idx}"));
        bind_idx += 1;
    }

    let cursor = params
        .active_cursor()
        .map(KeysetCursor::decode)
        .transpose()?;

    let (order, cursor_operator) = match params.direction() {
        CursorDirection::After => ("DESC", "<"),
        CursorDirection::Before => ("ASC", ">"),
    };

    if cursor.is_some() {
        let ts_idx = bind_idx;
        bind_idx += 1;
        let id_idx = bind_idx;
        bind_idx += 1;
        conditions.push(format!(
            "(created_at, id) {cursor_operator} (${}, ${})",
            ts_idx, id_idx
        ));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let data_sql = if params.uses_offset() {
        format!(
            "SELECT id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account \
             FROM tips {where_clause} \
             ORDER BY created_at DESC, id DESC \
             LIMIT ${} OFFSET ${}",
            bind_idx,
            bind_idx + 1
        )
    } else {
        format!(
            "SELECT id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account \
             FROM tips {where_clause} \
             ORDER BY created_at {order}, id {order} \
             LIMIT ${}",
            bind_idx
        )
    };

    macro_rules! bind_filters {
        ($q:expr) => {{
            let mut q = $q;
            if let Some(u) = username {
                q = q.bind(u);
            }
            if let Some(v) = min_amount {
                q = q.bind(v);
            }
            if let Some(v) = max_amount {
                q = q.bind(v);
            }
            if let Some(v) = from_date {
                q = q.bind(v);
            }
            if let Some(v) = to_date {
                q = q.bind(v);
            }
            q
        }};
    }

    let start = Instant::now();
    let mut q = bind_filters!(sqlx::query_as::<_, Tip>(&data_sql));
    if let Some(cursor) = cursor {
        q = q.bind(cursor.created_at).bind(cursor.id);
    }
    q = q.bind(params.limit + 1);
    if params.uses_offset() {
        q = q.bind(params.offset());
    }

    let mut tips: Vec<Tip> = q.fetch_all(&state.db).await?;
    if params.direction() == CursorDirection::Before && !params.uses_offset() {
        tips.reverse();
    }
    let duration = start.elapsed();
    DB_QUERY_DURATION_SECONDS
        .with_label_values(&["tips_keyset_paginated"])
        .observe(duration.as_secs_f64());

    Ok(PaginatedResponse::keyset(tips, params.limit, |tip| {
        KeysetCursor::new(tip.created_at, tip.id)
    }))
}

/// Report a tip message for moderation review.
pub async fn report_tip_message(
    state: &AppState,
    tip_id: Uuid,
    req: ReportMessageRequest,
) -> AppResult<()> {
    // Verify the tip exists and has a message.
    let tip = sqlx::query_as::<_, Tip>(
        "SELECT id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account \
         FROM tips WHERE id = $1",
    )
    .bind(tip_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Database(crate::errors::DatabaseError::NotFound {
        entity: "tip",
        identifier: tip_id.to_string(),
    }))?;

    let message_text = tip.message.as_deref().unwrap_or("");
    let reporter = req.reported_by.as_deref().unwrap_or("anonymous");

    state
        .moderation
        .flag("tip_message", tip_id, message_text, &req.reason, reporter)
        .await
        .map_err(|e| {
            tracing::error!("Failed to flag tip message: {e}");
            AppError::internal()
        })?;

    Ok(())
}

// ─────────────────── lower-level helper (for bulk operations) ────────────────

/// Lower-level tip recording within an existing transaction.
/// Retained for `TipService::bulk_record_tips`. Inserts as `pending_verification`.
pub async fn record_tip_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &RecordTipRequest,
) -> AppResult<Tip> {
    let tip = sqlx::query_as::<_, Tip>(
        r#"
        INSERT INTO tips
            (id, creator_username, amount, transaction_hash, tipper_source_account, status, created_at)
        VALUES ($1, $2, $3, $4, $5, 'pending_verification', NOW())
        ON CONFLICT (transaction_hash) DO NOTHING
        RETURNING id, creator_username, amount, transaction_hash, created_at, status, tipper_source_account
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&req.username)
    .bind(&req.amount)
    .bind(&req.transaction_hash)
    .bind(&req.tipper_source_account)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::Conflict {
        code: "DUPLICATE_TX_HASH",
        message: format!("Duplicate transaction hash: {}", req.transaction_hash),
    })?;

    Ok(tip)
}
