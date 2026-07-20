use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use super::performance::PerformanceMonitor;
use crate::config::AppConfig;
use crate::services::stellar_service::StellarService;
use crate::ws::TipEvent;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub stellar: StellarService,
    pub performance: Arc<PerformanceMonitor>,
    pub redis: Option<ConnectionManager>,
    pub broadcast_tx: broadcast::Sender<TipEvent>,
    /// Typed, validated application configuration loaded once at startup.
    /// Callers must use this instead of `std::env::var`.
    pub config: Arc<AppConfig>,
}

/// Connect to Postgres with simple retry on transient errors.
pub async fn connect(
    database_url: &str,
    max_connections: u32,
    min_connections: u32,
    acquire_timeout: Duration,
    max_retries: u32,
) -> Result<PgPool, sqlx::Error> {
    let base_delay = Duration::from_millis(500);
    let max_delay = Duration::from_secs(30);

    for attempt in 0..=max_retries {
        match PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(min_connections)
            .acquire_timeout(acquire_timeout)
            .connect(database_url)
            .await
        {
            Ok(pool) => {
                if attempt > 0 {
                    tracing::info!(attempt, "DB connection established after retries");
                }
                return Ok(pool);
            }
            Err(e) => {
                tracing::warn!(
                    attempt,
                    max_retries,
                    error = %e,
                    "DB connection attempt failed"
                );
                if attempt == max_retries {
                    return Err(e);
                }
                let delay = base_delay
                    .saturating_mul(2u32.saturating_pow(attempt))
                    .min(max_delay);
                tokio::time::sleep(delay).await;
            }
        }
    }
    Err(sqlx::Error::PoolTimedOut)
}
