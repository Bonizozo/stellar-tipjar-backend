use std::time::Instant;
use uuid::Uuid;

use crate::cache::{keys, redis_client};
use crate::controllers::{campaign_controller, team_controller};
use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::db::transaction;
use crate::errors::{AppError, AppResult};
use crate::metrics::collectors::{
    DB_QUERY_DURATION_SECONDS, TIPS_AMOUNT_XLM, TIPS_CREATED_TOTAL, TIPS_FAILED_TOTAL,
};
use crate::models::pagination::{PaginatedResponse, PaginationParams};
use crate::models::tip::{RecordTipRequest, Tip, TipFilters, TipSortParams};
use crate::moderation::ContentType;

#[tracing::instrument(skip(state), fields(username = %req.username, amount = %req.amount))]
pub async fn record_tip(state: &AppState, req: RecordTipRequest) -> AppResult<Tip> {
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

    let mut tx = transaction::begin_transaction(&state.db)
        .await
        .map_err(AppError::from)?;

    let start = Instant::now();
    // Pass state into the internal helper to support WebSocket broadcasting
    let (tip, match_result) = match record_tip_in_tx(state, &mut tx, &req).await {
        Ok(result) => result,
        Err(e) => {
            TIPS_FAILED_TOTAL
                .with_label_values(&["record_tip_in_tx"])
                .inc();
            return Err(e);
        }
    };
    tx.commit().await?;
    let duration = start.elapsed();

    DB_QUERY_DURATION_SECONDS
        .with_label_values(&["tip_atomic_record"])
        .observe(duration.as_secs_f64());
    TIPS_CREATED_TOTAL.inc();
    if let Ok(amount) = tip.amount.parse::<f64>() {
        TIPS_AMOUNT_XLM.observe(amount);
    }

    if let Some(match_info) = match_result {
        let db = state.db.clone();
        let username = tip.creator_username.clone();
        let tip_id = tip.id;
        tokio::spawn(async move {
            use crate::controllers::notification_controller;
            if let Ok(prefs) = notification_controller::get_preferences(&db, &username).await {
                if prefs.notify_on_tip {
                    let payload = serde_json::json!({
                        "tip_id": tip_id,
                        "matched_amount": match_info.matched_amount,
                        "campaign_id": match_info.campaign_id,
                        "sponsor_name": match_info.sponsor_name,
                    });
                    if let Err(e) = notification_controller::create_notification(
                        &db,
                        &username,
                        "campaign_matched",
                        payload,
                    )
                    .await
                    {
                        tracing::warn!("Failed to persist campaign match notification: {e}");
                    }
                }
            }
        });
    }

    QueryLogger::log_query("INSERT tips + tip_logs (transaction)", duration);
    state.performance.track_query("tip_atomic_record", duration);

    // Centralized cache invalidation for tips and leaderboards
    if let Some(ref inv) = state.invalidator {
        let tips_pattern = keys::creator_tips_pattern(&tip.creator_username);
        let _ = inv.invalidate_pattern(&tips_pattern).await;
        let _ = inv.invalidate_pattern("leaderboard:*").await;
        let _ = inv
            .invalidate_pattern(&keys::http_response_pattern("/tips"))
            .await;
        let _ = inv
            .invalidate_pattern(&keys::http_response_pattern("/creators/"))
            .await;
    }

    // Main branch added Webhooks
    crate::webhooks::trigger_webhooks(
        state.db.clone(),
        "tip.recorded",
        serde_json::to_value(&tip).unwrap(),
    )
    .await;
    // Notify external services via webhook.
    let payload = serde_json::to_value(&tip).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize tip webhook payload");
        crate::errors::AppError::internal()
    })?;
    crate::webhooks::trigger_webhooks(state.db.clone(), "tip.recorded", payload).await;

    // Audit log: tip recorded
    {
        let db = state.db.clone();
        let creator = tip.creator_username.clone();
        let tip_id = tip.id.to_string();
        let amount = tip.amount.clone();
        tokio::spawn(async move {
            let _ = crate::controllers::audit_log_controller::log(
                &db,
                "tip.recorded",
                None,
                "tip",
                Some(&tip_id),
                "create",
                None,
                Some(serde_json::json!({ "creator": creator, "amount": amount })),
                serde_json::json!({}),
                None,
                None,
            )
            .await;
        });
    }

    Ok(tip)
}

/// Lower-level tip recording that executes within an existing transaction.
pub async fn record_tip_in_tx(
    state: &AppState, // Added state parameter to fix scope issue in Main
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &RecordTipRequest,
) -> AppResult<(Tip, Option<crate::models::campaign::CampaignMatchResult>)> {
    let query_tip = r#"
        INSERT INTO tips (id, creator_username, amount, transaction_hash, message, created_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        RETURNING id, creator_username, amount, transaction_hash, message, created_at
        "#;

    let tip = sqlx::query_as::<_, Tip>(query_tip)
        .bind(Uuid::new_v4())
        .bind(&req.username)
        .bind(&req.amount)
        .bind(&req.transaction_hash)
        .bind(&req.message)
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

    // Apply team splits if the recipient belongs to a team.
    team_controller::record_tip_splits(
        &state,
        tip.id,
        &tip.creator_username,
        &tip.amount,
        &mut *tx,
    )
    .await?;

    // Broadcast to WebSocket (Main branch feature)
    let event = crate::ws::TipEvent {
        creator_id: tip.creator_username.clone(),
        tipper_id: req.transaction_hash.clone(),
        amount: tip.amount.parse::<u64>().unwrap_or(0),
        timestamp: tip.created_at.timestamp(),
    };
    crate::ws::broadcast_tip(&state.broadcast_tx, event).await;

    // Persist notification if creator has tip notifications enabled.
    {
        let db = state.db.clone();
        let username = tip.creator_username.clone();
        let payload = serde_json::json!({
            "tip_id": tip.id,
            "amount": tip.amount,
            "transaction_hash": tip.transaction_hash,
            "message": tip.message,
        });
        tokio::spawn(async move {
            use crate::controllers::notification_controller;
            match notification_controller::get_preferences(&db, &username).await {
                Ok(prefs) if prefs.notify_on_tip => {
                    if let Err(e) = notification_controller::create_notification(
                        &db,
                        &username,
                        "tip_received",
                        payload,
                    )
                    .await
                    {
                        tracing::warn!("Failed to persist tip notification: {e}");
                    }
                }
                _ => {}
            }
        });
    }

    // Attempt to apply a matching campaign to this tip.
    let match_result = campaign_controller::apply_tip_matching_campaign(
        &mut *tx,
        &tip.creator_username,
        tip.id,
        &tip.amount,
    )
    .await?;

    // Update tip goals progress and fire milestone notifications
    {
        let db = state.db.clone();
        let username = tip.creator_username.clone();
        let amount = tip.amount.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::controllers::goal_controller::apply_tip_to_goals(&db, &username, &amount)
                    .await
            {
                tracing::warn!("Failed to update goal progress: {e}");
            }
        });
    }

    Ok((tip, match_result))
}

/// Fetch all tips for a creator without pagination (kept for internal use).
pub async fn get_tips_for_creator(state: &AppState, username: &str) -> AppResult<Vec<Tip>> {
    let query = r#"
        SELECT id, creator_username, amount, transaction_hash, message, created_at
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
    sort: TipSortParams,
) -> AppResult<PaginatedResponse<Tip>> {
    let params = params.validated();
    let (sort_col, sort_dir) = sort.validated();

    // Extract filter values up front so we can reference them multiple times.
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

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM tips {where_clause}");
    let data_sql = format!(
        "SELECT id, creator_username, amount, transaction_hash, message, created_at \
         FROM tips {where_clause} \
         ORDER BY {sort_col} {sort_dir} \
         LIMIT ${bind_idx} OFFSET ${}",
        bind_idx + 1
    );

    // Bind all active filter parameters onto a query builder.
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

    let total: i64 = bind_filters!(sqlx::query_scalar(&count_sql))
        .fetch_one(&state.db)
        .await?;

    let start = Instant::now();
    let tips: Vec<Tip> = bind_filters!(sqlx::query_as::<_, Tip>(&data_sql))
        .bind(params.limit)
        .bind(params.offset())
        .fetch_all(&state.db)
        .await?;
    let duration = start.elapsed();

    DB_QUERY_DURATION_SECONDS
        .with_label_values(&["tips_paginated"])
        .observe(duration.as_secs_f64());

    Ok(PaginatedResponse::new(tips, total, &params))
}
