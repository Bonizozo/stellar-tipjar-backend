//! Background job processing system
//!
//! Provides async job queuing, worker pool, retry with exponential backoff,
//! scheduled/cron jobs, dead letter queue, priority levels, and metrics.

pub mod handlers;
pub mod monitoring;
pub mod queue;
pub mod scheduler;
pub mod types;
pub mod worker;

pub use handlers::{
    AggregateStatsHandler, CleanupDataHandler, JobHandlerRegistry, SendNotificationHandler,
    VerifyTransactionHandler,
};
pub use monitoring::JobMonitor;
pub use queue::{JobQueueManager, QueueMetrics};
pub use scheduler::JobScheduler;
pub use types::*;
pub use worker::JobWorkerPool;

use crate::db::connection::AppState;
use std::sync::Arc;

/// Initialise the full job system: queue, handlers, worker pool, and scheduler.
/// Returns the pool and scheduler so the caller can shut them down on exit.
pub fn start(state: Arc<AppState>, config: JobConfig) -> (Arc<JobQueueManager>, JobScheduler) {
    let queue = Arc::new(JobQueueManager::new(Arc::new(state.db.clone())));

    // Register handlers
    let mut registry = JobHandlerRegistry::new();
    registry.register(Box::new(VerifyTransactionHandler::new(Arc::clone(&state))));
    registry.register(Box::new(SendNotificationHandler::new(Arc::clone(&state))));
    registry.register(Box::new(AggregateStatsHandler::new(Arc::clone(&state))));
    registry.register(Box::new(CleanupDataHandler::new(Arc::clone(&state))));

    let registry = Arc::new(registry);
    let worker_config = WorkerConfig::from(&config);

    // Start worker pool
    let pool = JobWorkerPool::new(worker_config, Arc::clone(&queue), registry);
    pool.start();

    // Start scheduler
    let scheduler = JobScheduler::new(Arc::clone(&queue), config);
    scheduler.start();

    (queue, scheduler)
}
