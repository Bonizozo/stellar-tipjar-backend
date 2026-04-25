use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::services::stellar_service::StellarService;

/// Rolling stats for the monitoring dashboard.
#[derive(Debug, Default)]
pub struct MonitoringStats {
    pub transactions_checked: AtomicU64,
    pub transactions_verified: AtomicU64,
    pub transactions_failed: AtomicU64,
    pub network_errors: AtomicU64,
}

impl MonitoringStats {
    pub fn snapshot(&self) -> MonitoringSnapshot {
        MonitoringSnapshot {
            transactions_checked: self.transactions_checked.load(Ordering::Relaxed),
            transactions_verified: self.transactions_verified.load(Ordering::Relaxed),
            transactions_failed: self.transactions_failed.load(Ordering::Relaxed),
            network_errors: self.network_errors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct MonitoringSnapshot {
    pub transactions_checked: u64,
    pub transactions_verified: u64,
    pub transactions_failed: u64,
    pub network_errors: u64,
}

/// Monitors unverified tips in the database and attempts to verify them
/// against the Stellar network, auto-recording them when confirmed.
pub struct MonitoringService {
    stellar: StellarService,
    pub stats: Arc<MonitoringStats>,
    poll_interval: Duration,
    shutdown: Arc<RwLock<bool>>,
}

impl MonitoringService {
    pub fn new(stellar: StellarService, poll_interval: Duration) -> Self {
        Self {
            stellar,
            stats: Arc::new(MonitoringStats::default()),
            poll_interval,
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Signal the monitoring loop to stop.
    pub async fn stop(&self) {
        *self.shutdown.write().await = true;
    }

    /// Poll the database for pending (unverified) tips and verify each one
    /// against the Stellar Horizon API.
    pub async fn run(&self, state: Arc<AppState>) {
        info!(
            interval_secs = self.poll_interval.as_secs(),
            "Stellar transaction monitor started"
        );

        let mut ticker = interval(self.poll_interval);

        loop {
            ticker.tick().await;

            if *self.shutdown.read().await {
                info!("Stellar transaction monitor shutting down");
                break;
            }

            if let Err(e) = self.poll_pending_tips(&state).await {
                error!(error = %e, "Error polling pending tips");
            }
        }
    }

    async fn poll_pending_tips(&self, state: &Arc<AppState>) -> Result<(), sqlx::Error> {
        // Fetch tips that have not yet been confirmed.
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, transaction_hash FROM tips WHERE verified = false ORDER BY created_at ASC LIMIT 50",
        )
        .fetch_all(&state.db)
        .await?;

        if rows.is_empty() {
            return Ok(());
        }

        info!(count = rows.len(), "Checking unverified tips");

        for (id, hash) in rows {
            self.stats
                .transactions_checked
                .fetch_add(1, Ordering::Relaxed);

            let start = Instant::now();

            match self.stellar.verify_transaction(&hash).await {
                Ok(true) => {
                    self.stats
                        .transactions_verified
                        .fetch_add(1, Ordering::Relaxed);

                    sqlx::query("UPDATE tips SET verified = true WHERE id = $1")
                        .bind(id)
                        .execute(&state.db)
                        .await?;

                    info!(
                        tx_hash = %hash,
                        elapsed_ms = start.elapsed().as_millis(),
                        "Tip verified and marked confirmed"
                    );
                }
                Ok(false) => {
                    self.stats
                        .transactions_failed
                        .fetch_add(1, Ordering::Relaxed);

                    warn!(tx_hash = %hash, "Transaction not found or unsuccessful on Stellar");
                }
                Err(e) => {
                    self.stats
                        .network_errors
                        .fetch_add(1, Ordering::Relaxed);

                    error!(tx_hash = %hash, error = %e, "Network error verifying transaction");
                }
            }
        }

        Ok(())
    }
}

/// Spawn the monitoring loop as a background Tokio task.
/// Returns the service handle so callers can read stats or stop it.
pub fn spawn(state: Arc<AppState>) -> Arc<MonitoringService> {
    let poll_interval = Duration::from_secs(
        std::env::var("MONITORING_POLL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
    );

    let svc = Arc::new(MonitoringService::new(state.stellar.clone(), poll_interval));
    let svc_clone = Arc::clone(&svc);

    tokio::spawn(async move {
        svc_clone.run(state).await;
    });

    svc
}
