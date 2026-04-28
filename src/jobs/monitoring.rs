//! Background job monitoring — metrics, alerts, and dashboard.

use crate::jobs::queue::JobQueueManager;
use crate::metrics::collectors::{
    JOB_ALERTS_TOTAL, JOB_DURATION_SECONDS, JOB_FAILURES_TOTAL, JOB_QUEUE_DEPTH,
    JOB_SUCCESS_TOTAL,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

/// Thresholds that trigger alerts
pub struct AlertThresholds {
    pub max_pending_jobs: i64,
    pub max_failed_jobs: i64,
    pub max_queue_age_secs: i64,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            max_pending_jobs: 1000,
            max_failed_jobs: 50,
            max_queue_age_secs: 300,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JobAlert {
    pub level: &'static str,
    pub message: String,
    pub value: i64,
    pub threshold: i64,
}

#[derive(Debug, Serialize)]
pub struct JobDashboard {
    pub queue_depth: QueueDepth,
    pub recent_failures: Vec<RecentFailure>,
    pub alerts: Vec<JobAlert>,
    pub oldest_pending_age_secs: Option<i64>,
    pub snapshot_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct QueueDepth {
    pub pending: i64,
    pub running: i64,
    pub retrying: i64,
    pub completed: i64,
    pub failed: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecentFailure {
    pub id: uuid::Uuid,
    pub job_type: String,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub completed_at: Option<DateTime<Utc>>,
}

pub struct JobMonitor {
    pool: Arc<PgPool>,
    queue: Arc<JobQueueManager>,
    thresholds: AlertThresholds,
}

impl JobMonitor {
    pub fn new(pool: Arc<PgPool>, queue: Arc<JobQueueManager>) -> Arc<Self> {
        Arc::new(Self {
            pool,
            queue,
            thresholds: AlertThresholds::default(),
        })
    }

    /// Collect full dashboard snapshot
    pub async fn dashboard(&self) -> Result<JobDashboard, sqlx::Error> {
        let metrics = self.queue.queue_metrics().await.map_err(|e| {
            sqlx::Error::Protocol(e.to_string())
        })?;

        // Update Prometheus gauges
        JOB_QUEUE_DEPTH.with_label_values(&["pending"]).set(metrics.pending as f64);
        JOB_QUEUE_DEPTH.with_label_values(&["running"]).set(metrics.running as f64);
        JOB_QUEUE_DEPTH.with_label_values(&["retrying"]).set(metrics.retrying as f64);
        JOB_QUEUE_DEPTH.with_label_values(&["failed"]).set(metrics.failed as f64);

        let depth = QueueDepth {
            pending: metrics.pending,
            running: metrics.running,
            retrying: metrics.retrying,
            completed: metrics.completed,
            failed: metrics.failed,
        };

        let recent_failures = sqlx::query_as::<_, RecentFailure>(
            r#"
            SELECT id, job_type, error_message, retry_count, completed_at
            FROM jobs
            WHERE status = 'failed'
            ORDER BY completed_at DESC NULLS LAST
            LIMIT 20
            "#,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        let oldest_pending_age_secs: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT EXTRACT(EPOCH FROM (NOW() - MIN(scheduled_at)))::BIGINT
            FROM jobs
            WHERE status IN ('pending', 'retrying')
            "#,
        )
        .fetch_optional(self.pool.as_ref())
        .await?
        .flatten();

        let alerts = self.evaluate_alerts(&depth, oldest_pending_age_secs);

        Ok(JobDashboard {
            queue_depth: depth,
            recent_failures,
            alerts,
            oldest_pending_age_secs,
            snapshot_at: Utc::now(),
        })
    }

    fn evaluate_alerts(&self, depth: &QueueDepth, oldest_age: Option<i64>) -> Vec<JobAlert> {
        let mut alerts = Vec::new();

        if depth.pending > self.thresholds.max_pending_jobs {
            JOB_ALERTS_TOTAL.with_label_values(&["high_pending"]).inc();
            alerts.push(JobAlert {
                level: "warning",
                message: format!("Pending job queue depth is high: {}", depth.pending),
                value: depth.pending,
                threshold: self.thresholds.max_pending_jobs,
            });
        }

        if depth.failed > self.thresholds.max_failed_jobs {
            JOB_ALERTS_TOTAL.with_label_values(&["high_failures"]).inc();
            alerts.push(JobAlert {
                level: "critical",
                message: format!("Failed job count exceeds threshold: {}", depth.failed),
                value: depth.failed,
                threshold: self.thresholds.max_failed_jobs,
            });
        }

        if let Some(age) = oldest_age {
            if age > self.thresholds.max_queue_age_secs {
                JOB_ALERTS_TOTAL.with_label_values(&["stale_jobs"]).inc();
                alerts.push(JobAlert {
                    level: "warning",
                    message: format!("Oldest pending job is {}s old", age),
                    value: age,
                    threshold: self.thresholds.max_queue_age_secs,
                });
            }
        }

        alerts
    }

    /// Record job execution metrics (called by worker on completion/failure)
    pub fn record_success(job_type: &str, duration: Duration) {
        JOB_SUCCESS_TOTAL.with_label_values(&[job_type]).inc();
        JOB_DURATION_SECONDS
            .with_label_values(&[job_type])
            .observe(duration.as_secs_f64());
    }

    pub fn record_failure(job_type: &str, reason: &str) {
        JOB_FAILURES_TOTAL.with_label_values(&[job_type, reason]).inc();
    }

    /// Spawn background monitoring loop
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                match self.dashboard().await {
                    Ok(dash) => {
                        if !dash.alerts.is_empty() {
                            tracing::warn!(
                                alerts = dash.alerts.len(),
                                pending = dash.queue_depth.pending,
                                failed = dash.queue_depth.failed,
                                "Job monitoring alerts triggered"
                            );
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "Job monitor dashboard error"),
                }
            }
        });
    }
}
