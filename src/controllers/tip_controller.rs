use anyhow::Result;
use std::time::Instant;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::models::tip::{RecordTipRequest, Tip};
use crate::cache::{redis_client, keys};

use crate::db::transaction;

pub async fn record_tip(state: &AppState, req: RecordTipRequest) -> Result<Tip> {
    let mut tx = transaction::begin_transaction(&state.db).await?;
    
    let start = Instant::now();
    let tip = record_tip_in_tx(&mut tx, &req).await?;
    tx.commit().await?;
    let duration = start.elapsed();

    // Log the successful atomic operation
    QueryLogger::log_query("INSERT tips + tip_logs (transaction)", duration);
    state.performance.track_query("tip_atomic_record", duration);

    // Invalidate the tip list cache for this creator since it's now stale.
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let tips_key = keys::creator_tips(&tip.creator_username);
        let _ = redis_client::del(&mut conn, &[tips_key.as_str()]).await;
    }

    // Notify external services via webhook.
    crate::webhooks::trigger_webhooks(
        state.db.clone(), 
        "tip.recorded", 
        serde_json::to_value(&tip).unwrap()
    ).await;

    Ok(tip)
}

/// Lower-level tip recording that executes within an existing transaction.
/// This allows for multi-step atomic operations coordinated by a service.
pub async fn record_tip_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &RecordTipRequest,
) -> Result<Tip> {
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

    // Multi-step operation: Add to tip_logs
    let query_log = r#"
        INSERT INTO tip_logs (tip_id, creator_username, action)
        VALUES ($1, $2, 'recorded_atomic')
        "#;
    
    sqlx::query(query_log)
        .bind(&tip.id)
        .bind(&tip.creator_username)
        .execute(&mut **tx)
        .await?;

    Ok(tip)
}

pub async fn get_tips_for_creator(state: &AppState, username: &str) -> Result<Vec<Tip>> {
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

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);

    // Populate cache.
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let cache_key = keys::creator_tips(username);
        let _ = redis_client::set(&mut conn, &cache_key, &tips, redis_client::TTL_TIPS).await;
    }

    Ok(tips)
}
