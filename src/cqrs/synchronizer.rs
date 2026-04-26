use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use crate::errors::AppResult;

use super::projections::{CqrsProjection, ProjectionSyncReport};

#[derive(Clone)]
pub struct CqrsSynchronizer {
    projection: Arc<CqrsProjection>,
    last_sequence: Arc<AtomicI64>,
}

impl CqrsSynchronizer {
    pub fn new(projection: Arc<CqrsProjection>) -> Self {
        Self {
            projection,
            last_sequence: Arc::new(AtomicI64::new(0)),
        }
    }

    pub fn last_sequence(&self) -> i64 {
        self.last_sequence.load(Ordering::SeqCst)
    }

    pub async fn sync_once(&self) -> AppResult<ProjectionSyncReport> {
        let from = self.last_sequence();
        let report = self.projection.sync_from_sequence(from + 1).await?;
        if report.to_sequence > 0 {
            self.last_sequence
                .store(report.to_sequence, Ordering::SeqCst);
        }
        Ok(report)
    }

    /// Best-effort eventual consistency loop for read-model catch-up.
    pub async fn sync_with_retry(
        &self,
        max_attempts: usize,
        base_delay: Duration,
    ) -> AppResult<ProjectionSyncReport> {
        let mut attempts = 0usize;
        loop {
            attempts += 1;
            match self.sync_once().await {
                Ok(report) => return Ok(report),
                Err(e) if attempts < max_attempts => {
                    tracing::warn!(
                        attempt = attempts,
                        max_attempts,
                        error = %e,
                        "CQRS read-model sync failed, retrying"
                    );
                    sleep(base_delay.saturating_mul(attempts as u32)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
