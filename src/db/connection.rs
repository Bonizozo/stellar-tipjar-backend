use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use super::performance::PerformanceMonitor;
use super::replica::ReplicaManager;
use crate::cache::{CacheInvalidator, MultiLayerCache};
use crate::moderation::ModerationService;
use crate::services::circuit_breaker::CircuitBreaker;
use crate::services::distributed_lock::DistributedLockService;
use crate::services::stellar_service::StellarService;
use crate::ws::TipEvent;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub stellar: StellarService,
    pub performance: Arc<PerformanceMonitor>,
    pub redis: Option<ConnectionManager>,
    pub broadcast_tx: broadcast::Sender<TipEvent>,
    pub moderation: Arc<crate::moderation::ModerationService>,
    pub db_circuit_breaker: Arc<CircuitBreaker>,
    pub cache: Option<Arc<MultiLayerCache>>,
    pub invalidator: Option<Arc<CacheInvalidator>>,
    /// Read replica manager — None when no replicas are configured.
    pub replicas: Option<Arc<ReplicaManager>>,
    /// Distributed lock service — None when Redis is unavailable.
    pub lock_service: Option<Arc<DistributedLockService>>,
}

impl AppState {
    /// Returns a read pool (replica if available, primary as fallback).
    pub async fn read_pool(&self) -> PgPool {
        if let Some(ref mgr) = self.replicas {
            if let Some(pool) = mgr.get_replica().await {
                return pool;
            }
        }
        self.db.clone()
    }
}

/// Connect to Postgres with exponential-backoff retry and circuit-breaker protection.
///
/// Retries up to `max_retries` times with delays: 500 ms, 1 s, 2 s, 4 s, …
/// capped at 30 s. Pool exhaustion (PoolTimedOut) is treated as a retryable error.
/// The circuit breaker trips after `cb_threshold` consecutive failures and
/// prevents further attempts until `cb_recovery_secs` have elapsed.
pub async fn connect_with_retry(
    database_url: &str,
    max_connections: u32,
    min_connections: u32,
    acquire_timeout: Duration,
    max_retries: u32,
    cb_threshold: u32,
    cb_recovery_secs: u64,
) -> Result<PgPool, sqlx::Error> {
    let cb = CircuitBreaker::new(cb_threshold, Duration::from_secs(cb_recovery_secs));
    let base_delay = Duration::from_millis(500);
    let max_delay = Duration::from_secs(30);

    for attempt in 0..=max_retries {
        if !cb.allow_request() {
            tracing::error!(
                attempt,
                "DB circuit breaker is OPEN – refusing connection attempt"
            );
            return Err(sqlx::Error::PoolTimedOut);
        }

        match PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(min_connections)
            .acquire_timeout(acquire_timeout)
            .connect(database_url)
            .await
        {
            Ok(pool) => {
                cb.record_success();
                if attempt > 0 {
                    tracing::info!(attempt, "DB connection established after retries");
                }
                return Ok(pool);
            }
            Err(e) => {
                cb.record_failure();
                let retryable = is_retryable_connect_error(&e);
                tracing::warn!(
                    attempt,
                    max_retries,
                    retryable,
                    error = %e,
                    "DB connection attempt failed"
                );

                if attempt == max_retries || !retryable {
                    tracing::error!(
                        attempt,
                        error = %e,
                        "DB connection failed permanently after {} attempts",
                        attempt + 1
                    );
                    return Err(e);
                }

                let delay = base_delay
                    .saturating_mul(2u32.saturating_pow(attempt))
                    .min(max_delay);
                tracing::info!(delay_ms = delay.as_millis(), "Retrying DB connection");
                tokio::time::sleep(delay).await;
            }
        }
    }

    Err(sqlx::Error::PoolTimedOut)
}

fn is_retryable_connect_error(e: &sqlx::Error) -> bool {
    matches!(
        e,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) || matches!(e, sqlx::Error::Database(db_err)
        if db_err.code().map_or(false, |c| {
            // 57P03 = cannot_connect_now, 08006 = connection_failure
            c == "57P03" || c == "08006" || c == "08001" || c == "08004"
        }))
}
