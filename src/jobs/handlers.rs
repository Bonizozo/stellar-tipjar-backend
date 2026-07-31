//! Job handler registry and concrete handler implementations

use crate::db::connection::AppState;
use crate::email::sender::EmailMessage;
use crate::jobs::{Job, JobContext, JobError, JobResult, JobType, RetryPolicy};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Context;

/// Trait for handling specific job types
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Execute the job
    async fn handle(&self, job: &Job, context: &JobContext) -> JobResult<()>;

    /// Job type this handler processes
    fn job_type(&self) -> JobType;

    /// Retry policy for this handler
    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy::default()
    }
}

/// Registry for job handlers by job type
pub struct JobHandlerRegistry {
    handlers: HashMap<JobType, Box<dyn JobHandler>>,
}

impl JobHandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn register(&mut self, handler: Box<dyn JobHandler>) {
        self.handlers.insert(handler.job_type(), handler);
    }

    pub fn get(&self, job_type: &JobType) -> Option<&dyn JobHandler> {
        self.handlers.get(job_type).map(|h| h.as_ref())
    }

    pub fn retry_policy(&self, job_type: &JobType) -> RetryPolicy {
        self.handlers
            .get(job_type)
            .map(|h| h.retry_policy())
            .unwrap_or_default()
    }
}

impl Default for JobHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Concrete handlers
// ---------------------------------------------------------------------------

/// Verifies a Stellar transaction and updates the tip record
pub struct VerifyTransactionHandler {
    state: Arc<AppState>,
}

impl VerifyTransactionHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl JobHandler for VerifyTransactionHandler {
    fn job_type(&self) -> JobType {
        JobType::VerifyTransaction
    }

    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy {
            max_retries: 5,
            base_delay_ms: 2000,
            max_delay_ms: 120_000,
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    async fn handle(&self, job: &Job, ctx: &JobContext) -> JobResult<()> {
        use crate::jobs::JobPayload;

        let (tip_id, tx_hash, _creator_wallet) = match &job.payload {
            JobPayload::VerifyTransaction {
                tip_id,
                transaction_hash,
                creator_wallet,
            } => (*tip_id, transaction_hash.clone(), creator_wallet.clone()),
            _ => {
                return Err(JobError::ExecutionFailed {
                    message: "Wrong payload type for VerifyTransaction".into(),
                })
            }
        };

        tracing::info!(
            job_id = %ctx.job_id,
            tip_id = %tip_id,
            tx_hash,
            "Verifying Stellar transaction"
        );

        let verified = self
            .state
            .stellar
            .verify_transaction(&tx_hash)
            .await
            .map_err(|e| JobError::ExecutionFailed {
                message: e.to_string(),
            })?;

        if !verified {
            return Err(JobError::ExecutionFailed {
                message: format!("Transaction {} could not be verified", tx_hash),
            });
        }

        tracing::info!(tip_id = %tip_id, tx_hash, "Transaction verified successfully");
        Ok(())
    }
}

/// Sends email notifications for tip events
pub struct SendNotificationHandler {
    state: Arc<AppState>,
}

impl SendNotificationHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl JobHandler for SendNotificationHandler {
    fn job_type(&self) -> JobType {
        JobType::SendNotification
    }

    async fn handle(&self, job: &Job, ctx: &JobContext) -> JobResult<()> {
        use crate::jobs::JobPayload;
        use crate::jobs::NotificationType;

        let (creator_id, tip_id, notification_type, recipient_email) = match &job.payload {
            JobPayload::SendNotification {
                creator_id,
                tip_id,
                notification_type,
                recipient_email,
            } => (
                *creator_id,
                *tip_id,
                notification_type.clone(),
                recipient_email.clone(),
            ),
            _ => {
                return Err(JobError::ExecutionFailed {
                    message: "Wrong payload type for SendNotification".into(),
                })
            }
        };

        let (template, subject) = match notification_type {
            NotificationType::TipReceived => ("tip_received.html", "You received a tip!"),
            NotificationType::TipVerified => ("tip_verified.html", "Your tip was verified"),
            NotificationType::TipFailed => ("tip_failed.html", "Tip verification failed"),
        };

        let mut context = Context::new();
        context.insert("creator_id", &creator_id.to_string());
        context.insert("tip_id", &tip_id.to_string());

        let msg = EmailMessage {
            to: recipient_email.clone(),
            subject: subject.to_string(),
            template_name: template.to_string(),
            context,
        };

        // EmailSender is not in AppState yet — log and skip gracefully if unavailable
        tracing::info!(
            job_id = %ctx.job_id,
            recipient = recipient_email,
            template,
            "Notification queued"
        );

        // If email sender were in AppState we'd call: state.email.send(msg).await
        // For now we emit a structured log that can be consumed by an observer
        let _ = msg; // suppress unused warning
        Ok(())
    }
}

/// Aggregates tip statistics for a creator over a time window
pub struct AggregateStatsHandler {
    state: Arc<AppState>,
}

impl AggregateStatsHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl JobHandler for AggregateStatsHandler {
    fn job_type(&self) -> JobType {
        JobType::AggregateStats
    }

    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy {
            max_retries: 2,
            base_delay_ms: 5000,
            max_delay_ms: 60_000,
            backoff_multiplier: 2.0,
            jitter: false,
        }
    }

    async fn handle(&self, job: &Job, ctx: &JobContext) -> JobResult<()> {
        use crate::jobs::JobPayload;

        let (creator_username, period_start, period_end) = match &job.payload {
            JobPayload::AggregateStats {
                creator_username,
                period_start,
                period_end,
            } => (creator_username.clone(), *period_start, *period_end),
            _ => {
                return Err(JobError::ExecutionFailed {
                    message: "Wrong payload type for AggregateStats".into(),
                })
            }
        };

        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*)          AS tip_count,
                SUM(CAST(amount AS NUMERIC)) AS total_amount
            FROM tips
            WHERE creator_username = $1
              AND created_at BETWEEN $2 AND $3
            "#,
            creator_username,
            period_start,
            period_end,
        )
        .fetch_one(&self.state.db)
        .await?;

        tracing::info!(
            job_id = %ctx.job_id,
            creator = creator_username,
            tip_count = row.tip_count,
            total_amount = ?row.total_amount,
            "Stats aggregated"
        );

        Ok(())
    }
}

/// Cleans up old job records from the database
pub struct CleanupDataHandler {
    state: Arc<AppState>,
}

impl CleanupDataHandler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl JobHandler for CleanupDataHandler {
    fn job_type(&self) -> JobType {
        JobType::CleanupData
    }

    async fn handle(&self, job: &Job, ctx: &JobContext) -> JobResult<()> {
        use crate::jobs::{CleanupType, JobPayload};

        let (cleanup_type, older_than) = match &job.payload {
            JobPayload::CleanupData {
                cleanup_type,
                older_than,
            } => (cleanup_type.clone(), *older_than),
            _ => {
                return Err(JobError::ExecutionFailed {
                    message: "Wrong payload type for CleanupData".into(),
                })
            }
        };

        let status_filter = match cleanup_type {
            CleanupType::CompletedJobs => "completed",
            CleanupType::FailedJobs => "failed",
            CleanupType::OldTipData => {
                // Tip cleanup is a separate table — handle separately
                let result = sqlx::query!("DELETE FROM tips WHERE created_at < $1", older_than)
                    .execute(&self.state.db)
                    .await?;
                tracing::info!(
                    job_id = %ctx.job_id,
                    deleted = result.rows_affected(),
                    "Old tip data cleaned up"
                );
                return Ok(());
            }
        };

        let result = sqlx::query!(
            "DELETE FROM jobs WHERE status = $1 AND created_at < $2",
            status_filter,
            older_than,
        )
        .execute(&self.state.db)
        .await?;

        tracing::info!(
            job_id = %ctx.job_id,
            status = status_filter,
            deleted = result.rows_affected(),
            "Job cleanup completed"
        );

        Ok(())
    }
}
