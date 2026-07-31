pub mod connection;
pub mod consumer;
pub mod handlers;
pub mod publisher;
pub mod system;
pub mod worker;

// Flat re-exports for the most commonly used types.
pub use connection::RabbitMQConnection;
pub use consumer::{MessageConsumer, MessageHandler, MessageHandlerRegistry};
pub use handlers::{build_handler_registry, ExchangeNames, MessageTypes, QueueConfig, QueueNames};
pub use publisher::{DeadLetterMessage, Message, MessagePublisher};
pub use system::{try_start, QueueSystem};

use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A verification job payload queued for async processing.
#[derive(Debug, Clone)]
pub struct VerificationJob {
    /// The tip's database ID.
    pub tip_id: Uuid,
    /// The Stellar transaction hash to verify.
    pub transaction_hash: String,
    /// Claimed amount in stroops.
    pub amount_stroops: i64,
    /// Creator's Stellar wallet address (payment destination).
    pub destination: String,
    /// Optional memo the tipper should have included.
    pub expected_memo: Option<String>,
    /// Claimed source (tipper's Stellar account).
    pub source_account: String,
    /// How many times this job has been attempted.
    pub attempt: u32,
}

/// A cloneable sender handle for enqueuing verification jobs.
#[derive(Clone)]
pub struct VerificationQueue {
    sender: mpsc::Sender<VerificationJob>,
}

impl VerificationQueue {
    /// Maximum number of jobs that can wait in the channel before back-pressure.
    const CHANNEL_CAPACITY: usize = 1_024;

    /// Create a new queue and return the sender half plus the receiver for the worker.
    pub fn new() -> (Self, mpsc::Receiver<VerificationJob>) {
        let (sender, receiver) = mpsc::channel(Self::CHANNEL_CAPACITY);
        (Self { sender }, receiver)
    }

    /// Enqueue a job for background verification.
    ///
    /// Returns `Ok(())` if the job was accepted; `Err` if the channel is full
    /// or closed, which callers should treat as a best-effort degradation path
    /// (the reconciliation job will pick it up later).
    pub async fn enqueue(&self, job: VerificationJob) -> Result<(), String> {
        self.sender
            .send(job)
            .await
            .map_err(|e| format!("Verification queue full or closed: {}", e))
    }

    /// Try to enqueue without waiting. Returns immediately if channel is full.
    pub fn try_enqueue(&self, job: VerificationJob) -> Result<(), String> {
        self.sender
            .try_send(job)
            .map_err(|e| format!("Verification queue try_send failed: {}", e))
    }
}

// Allow constructing a VerificationQueue in tests easily
impl std::fmt::Debug for VerificationQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerificationQueue").finish()
    }
}
