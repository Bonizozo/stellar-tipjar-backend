use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::models::tx_pool::{EnqueueTxRequest, TxPool, TxPoolStats, TxPoolStatus};

/// Enqueue a new transaction. Returns the created pool entry.
pub async fn enqueue(state: &AppState, req: EnqueueTxRequest) -> AppResult<TxPool> {
    let max_retries = req.max_retries.unwrap_or(5).clamp(1, 10);
    let row = sqlx::query_as::<_, TxPool>(
        r#"
        INSERT INTO tx_pool (transaction_hash, metadata, max_retries)
        VALUES ($1, $2, $3)
        ON CONFLICT (transaction_hash) DO UPDATE
            SET status       = 'pending',
                retry_count  = 0,
                last_error   = NULL,
                updated_at   = NOW(),
                next_retry_at = NOW()
        RETURNING *
        "#,
    )
    .bind(&req.transaction_hash)
    .bind(&req.metadata)
    .bind(max_retries)
    .fetch_one(&state.db)
    .await?;

    Ok(row)
}

/// Get a pool entry by transaction hash.
pub async fn get_by_hash(state: &AppState, tx_hash: &str) -> AppResult<Option<TxPool>> {
    let row = sqlx::query_as::<_, TxPool>("SELECT * FROM tx_pool WHERE transaction_hash = $1")
        .bind(tx_hash)
        .fetch_optional(&state.db)
        .await?;
    Ok(row)
}

/// Get a pool entry by id.
pub async fn get_by_id(state: &AppState, id: Uuid) -> AppResult<Option<TxPool>> {
    let row = sqlx::query_as::<_, TxPool>("SELECT * FROM tx_pool WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    Ok(row)
}

/// Aggregate stats across all statuses.
pub async fn stats(state: &AppState) -> AppResult<TxPoolStats> {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE status = 'pending'),
            COUNT(*) FILTER (WHERE status = 'processing'),
            COUNT(*) FILTER (WHERE status = 'confirmed'),
            COUNT(*) FILTER (WHERE status = 'failed')
        FROM tx_pool
        "#,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(TxPoolStats {
        pending: row.0,
        processing: row.1,
        confirmed: row.2,
        failed: row.3,
    })
}

// ── Background worker ────────────────────────────────────────────────────────

/// Claim up to `batch` pending entries whose next_retry_at is due,
/// mark them processing, verify on Stellar, then update status.
async fn process_batch(state: &Arc<AppState>, batch: i64) {
    // Claim pending entries atomically.
    let rows: Vec<TxPool> = match sqlx::query_as::<_, TxPool>(
        r#"
        UPDATE tx_pool
        SET status = 'processing', updated_at = NOW()
        WHERE id IN (
            SELECT id FROM tx_pool
            WHERE status = 'pending' AND next_retry_at <= NOW()
            ORDER BY next_retry_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING *
        "#,
    )
    .bind(batch)
    .fetch_all(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "tx_pool: failed to claim batch");
            return;
        }
    };

    if rows.is_empty() {
        return;
    }

    info!(count = rows.len(), "tx_pool: processing batch");

    for entry in rows {
        let result = state.stellar.verify_transaction(&entry.transaction_hash).await;

        match result {
            Ok(true) => {
                // Confirmed.
                if let Err(e) = sqlx::query(
                    "UPDATE tx_pool SET status = 'confirmed', updated_at = NOW() WHERE id = $1",
                )
                .bind(entry.id)
                .execute(&state.db)
                .await
                {
                    error!(id = %entry.id, error = %e, "tx_pool: failed to mark confirmed");
                }
                info!(tx_hash = %entry.transaction_hash, "tx_pool: confirmed");
            }
            Ok(false) | Err(_) => {
                let err_msg = match result {
                    Err(ref e) => e.to_string(),
                    _ => "transaction not found or unsuccessful".to_string(),
                };

                let new_retry = entry.retry_count + 1;
                if new_retry >= entry.max_retries {
                    // Exhausted retries → failed.
                    if let Err(e) = sqlx::query(
                        "UPDATE tx_pool SET status = 'failed', retry_count = $2, last_error = $3, updated_at = NOW() WHERE id = $1",
                    )
                    .bind(entry.id)
                    .bind(new_retry)
                    .bind(&err_msg)
                    .execute(&state.db)
                    .await
                    {
                        error!(id = %entry.id, error = %e, "tx_pool: failed to mark failed");
                    }
                    warn!(tx_hash = %entry.transaction_hash, retries = new_retry, "tx_pool: exhausted retries");
                } else {
                    // Schedule next retry with exponential backoff (30s * 2^retry, max 1h).
                    let backoff_secs = (30u64 * 2u64.saturating_pow(new_retry as u32)).min(3600);
                    if let Err(e) = sqlx::query(
                        r#"UPDATE tx_pool
                           SET status = 'pending',
                               retry_count = $2,
                               last_error = $3,
                               updated_at = NOW(),
                               next_retry_at = NOW() + ($4 || ' seconds')::interval
                           WHERE id = $1"#,
                    )
                    .bind(entry.id)
                    .bind(new_retry)
                    .bind(&err_msg)
                    .bind(backoff_secs.to_string())
                    .execute(&state.db)
                    .await
                    {
                        error!(id = %entry.id, error = %e, "tx_pool: failed to reschedule");
                    }
                    warn!(
                        tx_hash = %entry.transaction_hash,
                        retry = new_retry,
                        backoff_secs,
                        "tx_pool: scheduled retry"
                    );
                }
            }
        }
    }
}

/// Spawn the transaction pool worker as a background Tokio task.
pub fn spawn(state: Arc<AppState>) {
    let poll_secs: u64 = std::env::var("TX_POOL_POLL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);

    let batch: i64 = std::env::var("TX_POOL_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    tokio::spawn(async move {
        info!(poll_secs, batch, "tx_pool worker started");
        let mut ticker = interval(Duration::from_secs(poll_secs));
        loop {
            ticker.tick().await;
            process_batch(&state, batch).await;
        }
    });
}
