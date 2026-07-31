//! Job queue management and database operations

use crate::jobs::{
    Job, JobError, JobId, JobPayload, JobResult, JobRow, JobStatus, JobType, RetryPolicy, WorkerId,
};
use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Manages job lifecycle and database operations
pub struct JobQueueManager {
    pool: Arc<PgPool>,
}

impl JobQueueManager {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Add a new job to the queue
    pub async fn enqueue(
        &self,
        job_type: JobType,
        payload: JobPayload,
        priority: i32,
        max_retries: i32,
    ) -> JobResult<JobId> {
        let id = Uuid::new_v4();
        let job_type_str = job_type.to_string();
        let payload_json = serde_json::to_value(&payload)?;

        sqlx::query!(
            r#"
            INSERT INTO jobs (id, job_type, payload, status, priority, max_retries, scheduled_at)
            VALUES ($1, $2, $3, 'pending', $4, $5, NOW())
            "#,
            id,
            job_type_str,
            payload_json,
            priority,
            max_retries,
        )
        .execute(self.pool.as_ref())
        .await?;

        tracing::info!(job_id = %id, job_type = %job_type_str, "Job enqueued");
        Ok(id)
    }

    /// Get the next available job for processing (SKIP LOCKED for concurrent workers)
    pub async fn dequeue(&self, worker_id: WorkerId) -> JobResult<Option<Job>> {
        let row = sqlx::query_as!(
            JobRow,
            r#"
            UPDATE jobs
            SET status = 'running',
                started_at = NOW(),
                worker_id = $1,
                retry_count = retry_count + 1
            WHERE id = (
                SELECT id FROM jobs
                WHERE status IN ('pending', 'retrying')
                  AND scheduled_at <= NOW()
                ORDER BY priority DESC NULLS LAST, scheduled_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING id, job_type, payload, status, retry_count, max_retries,
                      created_at, scheduled_at, started_at, completed_at,
                      error_message, worker_id, priority
            "#,
            worker_id,
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(Job::try_from).transpose()
    }

    /// Mark a job as completed
    pub async fn complete(&self, job_id: JobId) -> JobResult<()> {
        sqlx::query!(
            "UPDATE jobs SET status = 'completed', completed_at = NOW() WHERE id = $1",
            job_id
        )
        .execute(self.pool.as_ref())
        .await?;

        tracing::info!(job_id = %job_id, "Job completed");
        Ok(())
    }

    /// Mark a job as failed, scheduling retry with exponential backoff if attempts remain
    pub async fn fail(&self, job_id: JobId, error: String, policy: &RetryPolicy) -> JobResult<()> {
        // Fetch current attempt count
        let row = sqlx::query!(
            "SELECT retry_count, max_retries FROM jobs WHERE id = $1",
            job_id
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        let attempts = row.retry_count;
        let max = row.max_retries;

        if attempts < max {
            let delay_secs = policy.next_delay_secs(attempts);
            sqlx::query!(
                r#"
                UPDATE jobs
                SET status = 'retrying',
                    error_message = $2,
                    scheduled_at = NOW() + ($3 || ' seconds')::interval
                WHERE id = $1
                "#,
                job_id,
                error,
                delay_secs.to_string(),
            )
            .execute(self.pool.as_ref())
            .await?;

            tracing::warn!(
                job_id = %job_id,
                attempt = attempts,
                max_retries = max,
                delay_secs,
                "Job failed, scheduled for retry"
            );
        } else {
            sqlx::query!(
                "UPDATE jobs SET status = 'failed', error_message = $2, completed_at = NOW() WHERE id = $1",
                job_id,
                error,
            )
            .execute(self.pool.as_ref())
            .await?;

            tracing::error!(job_id = %job_id, error, "Job permanently failed (dead letter)");
        }

        Ok(())
    }

    /// Get jobs in the dead letter queue (permanently failed)
    pub async fn dead_letter_jobs(&self, limit: i64) -> JobResult<Vec<Job>> {
        let rows = sqlx::query_as!(
            JobRow,
            r#"
            SELECT id, job_type, payload, status, retry_count, max_retries,
                   created_at, scheduled_at, started_at, completed_at,
                   error_message, worker_id, priority
            FROM jobs
            WHERE status = 'failed'
            ORDER BY completed_at DESC
            LIMIT $1
            "#,
            limit,
        )
        .fetch_all(self.pool.as_ref())
        .await?;

        rows.into_iter().map(Job::try_from).collect()
    }

    /// Clean up old completed/failed jobs beyond retention period
    pub async fn cleanup_old_jobs(&self, retention_days: i64) -> JobResult<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM jobs
            WHERE status IN ('completed', 'failed')
              AND created_at < NOW() - ($1 || ' days')::interval
            "#,
            retention_days.to_string(),
        )
        .execute(self.pool.as_ref())
        .await?;

        let deleted = result.rows_affected();
        tracing::info!(deleted, "Cleaned up old jobs");
        Ok(deleted)
    }

    /// Get queue depth metrics by status
    pub async fn queue_metrics(&self) -> JobResult<QueueMetrics> {
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE status = 'pending')   AS pending,
                COUNT(*) FILTER (WHERE status = 'running')   AS running,
                COUNT(*) FILTER (WHERE status = 'retrying')  AS retrying,
                COUNT(*) FILTER (WHERE status = 'completed') AS completed,
                COUNT(*) FILTER (WHERE status = 'failed')    AS failed
            FROM jobs
            "#
        )
        .fetch_one(self.pool.as_ref())
        .await?;

        Ok(QueueMetrics {
            pending: row.pending.unwrap_or(0),
            running: row.running.unwrap_or(0),
            retrying: row.retrying.unwrap_or(0),
            completed: row.completed.unwrap_or(0),
            failed: row.failed.unwrap_or(0),
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct QueueMetrics {
    pub pending: i64,
    pub running: i64,
    pub retrying: i64,
    pub completed: i64,
    pub failed: i64,
}
