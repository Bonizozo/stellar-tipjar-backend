//! Periodic job scheduling (cron-style)

use crate::jobs::{CleanupType, JobConfig, JobPayload, JobQueueManager, JobResult, JobType};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// Schedules periodic maintenance jobs on a fixed interval
pub struct JobScheduler {
    queue: Arc<JobQueueManager>,
    config: JobConfig,
    shutdown_tx: broadcast::Sender<()>,
}

impl JobScheduler {
    pub fn new(queue: Arc<JobQueueManager>, config: JobConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            queue,
            config,
            shutdown_tx,
        }
    }

    /// Spawn the scheduler as a background task
    pub fn start(&self) {
        let queue = Arc::clone(&self.queue);
        let cleanup_interval = Duration::from_secs(self.config.cleanup_interval_hours * 3600);
        let retention_days = self.config.job_retention_days;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            tracing::info!("Job scheduler started");
            let mut interval = tokio::time::interval(cleanup_interval);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        schedule_cleanup(&queue, retention_days).await;
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Job scheduler shutting down");
                        break;
                    }
                }
            }
        });
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

async fn schedule_cleanup(queue: &JobQueueManager, retention_days: i64) {
    let older_than = Utc::now() - chrono::Duration::days(retention_days);

    for cleanup_type in [CleanupType::CompletedJobs, CleanupType::FailedJobs] {
        let payload = JobPayload::CleanupData {
            cleanup_type,
            older_than,
        };

        if let Err(e) = queue.enqueue(JobType::CleanupData, payload, -10, 1).await {
            tracing::error!(error = %e, "Failed to schedule cleanup job");
        }
    }

    tracing::info!("Scheduled periodic cleanup jobs");
}
