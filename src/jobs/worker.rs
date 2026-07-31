//! Job worker implementation and worker pool management

use crate::jobs::handlers::JobHandlerRegistry;
use crate::jobs::types::JobContext;
use crate::jobs::{JobError, JobQueueManager, JobResult, WorkerConfig, WorkerId};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::Instrument;

/// Individual job worker that pulls and processes jobs from the queue
pub struct JobWorker {
    id: WorkerId,
    queue: Arc<JobQueueManager>,
    registry: Arc<JobHandlerRegistry>,
    poll_interval: Duration,
    shutdown_rx: broadcast::Receiver<()>,
}

impl JobWorker {
    pub fn new(
        id: WorkerId,
        queue: Arc<JobQueueManager>,
        registry: Arc<JobHandlerRegistry>,
        poll_interval: Duration,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            id,
            queue,
            registry,
            poll_interval,
            shutdown_rx,
        }
    }

    /// Run the worker loop until a shutdown signal is received
    pub async fn run(mut self) -> JobResult<()> {
        tracing::info!(worker_id = %self.id, "Worker started");

        loop {
            // Check for shutdown signal (non-blocking)
            match self.shutdown_rx.try_recv() {
                Ok(_) | Err(broadcast::error::TryRecvError::Closed) => {
                    tracing::info!(worker_id = %self.id, "Worker shutting down");
                    return Ok(());
                }
                Err(broadcast::error::TryRecvError::Empty) => {}
                Err(broadcast::error::TryRecvError::Lagged(_)) => {}
            }

            match self.queue.dequeue(self.id.clone()).await {
                Ok(Some(job)) => {
                    // Open an OTel-aware span for this job execution so it
                    // appears as a named node in the trace waterfall.
                    let job_span = tracing::info_span!(
                        "job.execute",
                        "job.id"      = %job.id,
                        "job.type"    = %job.job_type,
                        "worker.id"   = %self.id,
                        "retry_count" = job.retry_count,
                    );

                    let ctx = JobContext {
                        job_id: job.id,
                        worker_id: self.id.clone(),
                        retry_count: job.retry_count,
                        created_at: Utc::now(),
                    };

                    let handler = match self.registry.get(&job.job_type) {
                        Some(h) => h,
                        None => {
                            let err = format!("No handler for job type: {}", job.job_type);
                            tracing::error!(worker_id = %self.id, job_id = %job.id, "{}", err);
                            let policy = self.registry.retry_policy(&job.job_type);
                            let _ = self.queue.fail(job.id, err, &policy).await;
                            continue;
                        }
                    };

                    tracing::debug!(
                        worker_id = %self.id,
                        job_id = %job.id,
                        job_type = %job.job_type,
                        "Processing job"
                    );

                    let result = handler
                        .handle(&job, &ctx)
                        .instrument(job_span.clone())
                        .await;
                    match result {
                        Ok(()) => {
                            if let Err(e) = self.queue.complete(job.id).await {
                                tracing::error!(job_id = %job.id, error = %e, "Failed to mark job complete");
                            }
                        }
                        Err(JobError::Shutdown) => {
                            tracing::info!(worker_id = %self.id, "Shutdown during job execution");
                            return Ok(());
                        }
                        Err(e) => {
                            tracing::warn!(
                                worker_id = %self.id,
                                job_id = %job.id,
                                error = %e,
                                "Job execution failed"
                            );
                            let policy = self.registry.retry_policy(&job.job_type);
                            if let Err(db_err) =
                                self.queue.fail(job.id, e.to_string(), &policy).await
                            {
                                tracing::error!(job_id = %job.id, error = %db_err, "Failed to record job failure");
                            }
                        }
                    }
                }
                Ok(None) => {
                    // No jobs available — wait before polling again
                    tokio::time::sleep(self.poll_interval).await;
                }
                Err(e) => {
                    tracing::error!(worker_id = %self.id, error = %e, "Error dequeuing job");
                    tokio::time::sleep(self.poll_interval * 5).await;
                }
            }
        }
    }
}

/// Manages a pool of job workers
pub struct JobWorkerPool {
    config: WorkerConfig,
    queue: Arc<JobQueueManager>,
    registry: Arc<JobHandlerRegistry>,
    shutdown_tx: broadcast::Sender<()>,
}

impl JobWorkerPool {
    pub fn new(
        config: WorkerConfig,
        queue: Arc<JobQueueManager>,
        registry: Arc<JobHandlerRegistry>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            queue,
            registry,
            shutdown_tx,
        }
    }

    /// Spawn all workers as background tasks
    pub fn start(&self) {
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);

        for i in 0..self.config.worker_count {
            let worker = JobWorker::new(
                format!("worker-{}", i),
                Arc::clone(&self.queue),
                Arc::clone(&self.registry),
                poll_interval,
                self.shutdown_tx.subscribe(),
            );

            tokio::spawn(async move {
                if let Err(e) = worker.run().await {
                    tracing::error!(worker_id = %format!("worker-{}", i), error = %e, "Worker exited with error");
                }
            });
        }

        tracing::info!(count = self.config.worker_count, "Job worker pool started");
    }

    /// Signal all workers to stop
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        tracing::info!("Job worker pool shutdown signal sent");
    }

    pub fn worker_count(&self) -> usize {
        self.config.worker_count
    }
}
