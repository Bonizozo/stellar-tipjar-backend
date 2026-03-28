//! Job handler registry and trait definitions

use crate::jobs::{Job, JobContext, JobError, JobResult, JobType, RetryPolicy};
use async_trait::async_trait;
use std::collections::HashMap;

/// Trait for handling specific job types
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Handle a job execution
    async fn handle(&self, job: Job, context: &JobContext) -> JobResult<()>;

    /// Get the job type this handler processes
    fn job_type(&self) -> JobType;

    /// Get the retry policy for this job type
    fn retry_policy(&self) -> RetryPolicy;
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

    /// Register a handler for a specific job type
    pub fn register(&mut self, handler: Box<dyn JobHandler>) {
        let job_type = handler.job_type();
        self.handlers.insert(job_type, handler);
    }

    /// Get a handler for a job type
    pub fn get_handler(&self, job_type: &JobType) -> Option<&dyn JobHandler> {
        self.handlers.get(job_type).map(|h| h.as_ref())
    }

    /// Check if a handler is registered for a job type
    pub fn has_handler(&self, job_type: &JobType) -> bool {
        self.handlers.contains_key(job_type)
    }
}

impl Default for JobHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
