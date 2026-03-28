//! Job system type definitions and data models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a job
pub type JobId = Uuid;

/// Unique identifier for a worker
pub type WorkerId = String;

/// Main job entity representing a background task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub job_type: JobType,
    pub payload: JobPayload,
    pub status: JobStatus,
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: DateTime<Utc>,
    pub scheduled_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub worker_id: Option<WorkerId>,
}

/// Types of jobs that can be processed
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(type_name = "varchar", rename_all = "snake_case")]
pub enum JobType {
    VerifyTransaction,
    SendNotification,
    CleanupData,
}

/// Current status of a job
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "varchar", rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Retrying,
    Cancelled,
}

/// Type-safe job payloads for different job types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum JobPayload {
    VerifyTransaction {
        tip_id: Uuid,
        transaction_hash: String,
        creator_wallet: String,
    },
    SendNotification {
        creator_id: Uuid,
        tip_id: Uuid,
        notification_type: NotificationType,
    },
    CleanupData {
        cleanup_type: CleanupType,
        older_than: DateTime<Utc>,
    },
}

/// Types of notifications that can be sent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationType {
    TipReceived,
    TipVerified,
    TipFailed,
}

/// Types of data cleanup operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CleanupType {
    CompletedJobs,
    FailedJobs,
    OldTipData,
}

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: i32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 60000,
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Job system configuration
#[derive(Debug, Clone)]
pub struct JobConfig {
    pub worker_count: usize,
    pub poll_interval_ms: u64,
    pub shutdown_timeout_ms: u64,
    pub cleanup_interval_hours: u64,
    pub job_retention_days: i64,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            worker_count: 4,
            poll_interval_ms: 1000,
            shutdown_timeout_ms: 30000,
            cleanup_interval_hours: 24,
            job_retention_days: 7,
        }
    }
}

/// Worker configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_count: usize,
    pub poll_interval_ms: u64,
    pub shutdown_timeout_ms: u64,
}

impl From<&JobConfig> for WorkerConfig {
    fn from(config: &JobConfig) -> Self {
        Self {
            worker_count: config.worker_count,
            poll_interval_ms: config.poll_interval_ms,
            shutdown_timeout_ms: config.shutdown_timeout_ms,
        }
    }
}

/// Context provided to job handlers during execution
#[derive(Debug, Clone)]
pub struct JobContext {
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub retry_count: i32,
    pub created_at: DateTime<Utc>,
}

/// Result type for job operations
pub type JobResult<T> = Result<T, JobError>;

/// Errors that can occur during job processing
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Job handler not found for type: {job_type:?}")]
    HandlerNotFound { job_type: JobType },

    #[error("Job execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("External service unavailable: {service}")]
    ServiceUnavailable { service: String },

    #[error("Job timeout after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    #[error("Worker shutdown requested")]
    Shutdown,

    #[error("Invalid job state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: JobStatus, to: JobStatus },
}
