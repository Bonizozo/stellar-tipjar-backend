use super::connection::RabbitMQConnection;
use super::publisher::{DeadLetterMessage, Message};
use async_trait::async_trait;
use futures_lite::StreamExt;
use lapin::{
    message::Delivery,
    options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, BasicQosOptions},
    BasicProperties,
};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// ── Handler trait ─────────────────────────────────────────────────────────────

/// Implement this trait to handle a specific `message_type`.
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// Process the message.  Return `Ok(())` to ack, `Err(_)` to nack/retry.
    async fn handle(&self, message: &Message) -> anyhow::Result<()>;

    /// The `message_type` string this handler is responsible for.
    fn message_type(&self) -> &str;
}

// ── Handler registry ──────────────────────────────────────────────────────────

/// Routes incoming messages to the correct `MessageHandler` by `message_type`.
pub struct MessageHandlerRegistry {
    handlers: std::collections::HashMap<String, Box<dyn MessageHandler>>,
}

impl MessageHandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
        }
    }

    pub fn register(&mut self, handler: Box<dyn MessageHandler>) {
        self.handlers
            .insert(handler.message_type().to_string(), handler);
    }

    pub async fn dispatch(&self, message: &Message) -> anyhow::Result<()> {
        match self.handlers.get(&message.message_type) {
            Some(handler) => handler.handle(message).await,
            None => {
                tracing::warn!(
                    message.id = %message.id,
                    message.type = %message.message_type,
                    "No handler registered for message type"
                );
                // Ack unknown types so they don't loop forever.
                Ok(())
            }
        }
    }
}

// ── Consumer ──────────────────────────────────────────────────────────────────

/// Consumes messages from a single RabbitMQ queue.
///
/// Retry strategy:
/// - On handler error the message is nack'd with `requeue = false` so the
///   broker routes it to the DLX/DLQ (configured during topology setup).
/// - The consumer tracks `retry_count` inside the message envelope and
///   re-publishes to the *main* queue with an exponential delay (via a
///   per-message TTL queue) until `max_retries` is reached, at which point
///   the message is published directly to the DLQ.
pub struct MessageConsumer {
    connection: Arc<RabbitMQConnection>,
    queue_name: String,
    dlq_name: String,
    max_retries: u32,
    /// How many unacknowledged messages the broker may push at once.
    prefetch_count: u16,
}

impl MessageConsumer {
    pub fn new(
        connection: Arc<RabbitMQConnection>,
        queue_name: impl Into<String>,
        max_retries: u32,
        prefetch_count: u16,
    ) -> Self {
        let queue_name = queue_name.into();
        let dlq_name = format!("{}.dlq", queue_name);
        Self {
            connection,
            queue_name,
            dlq_name,
            max_retries,
            prefetch_count,
        }
    }

    /// Start the consumer loop.  Runs until `shutdown_rx` fires.
    ///
    /// Each delivery is dispatched to the registry.  On success the message is
    /// acked; on failure it is retried with exponential backoff or dead-lettered.
    pub async fn run(
        &self,
        registry: Arc<MessageHandlerRegistry>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        loop {
            // Re-establish the channel on every outer loop iteration so a
            // channel-level error (e.g. nack of a non-existent tag) doesn't
            // kill the whole consumer.
            let channel = match self.connection.create_channel().await {
                Ok(ch) => ch,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create consumer channel, retrying in 5 s");
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                        _ = shutdown_rx.recv() => { tracing::info!("Consumer shutting down"); return; }
                    }
                }
            };

            // Set QoS / prefetch so we don't buffer the whole queue in memory.
            if let Err(e) = channel
                .basic_qos(self.prefetch_count, BasicQosOptions::default())
                .await
            {
                tracing::error!(error = %e, "Failed to set QoS");
                continue;
            }

            let mut consumer = match channel
                .basic_consume(
                    &self.queue_name,
                    &format!("consumer-{}", Uuid::new_v4()),
                    BasicConsumeOptions::default(),
                    Default::default(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, queue = %self.queue_name, "Failed to start consumer, retrying in 5 s");
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                        _ = shutdown_rx.recv() => { tracing::info!("Consumer shutting down"); return; }
                    }
                }
            };

            tracing::info!(queue = %self.queue_name, "Consumer started");

            loop {
                tokio::select! {
                    delivery = consumer.next() => {
                        match delivery {
                            Some(Ok(delivery)) => {
                                self.process_delivery(&channel, delivery, &registry).await;
                            }
                            Some(Err(e)) => {
                                tracing::error!(error = %e, "Consumer stream error, reconnecting");
                                break; // break inner loop → reconnect
                            }
                            None => {
                                tracing::warn!("Consumer stream ended, reconnecting");
                                break;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!(queue = %self.queue_name, "Consumer received shutdown signal");
                        return;
                    }
                }
            }
        }
    }

    /// Process a single AMQP delivery.
    async fn process_delivery(
        &self,
        channel: &lapin::Channel,
        delivery: Delivery,
        registry: &Arc<MessageHandlerRegistry>,
    ) {
        let tag = delivery.delivery_tag;

        // Deserialise the envelope.
        let mut message: Message = match serde_json::from_slice(&delivery.data) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error = %e, "Failed to deserialise message, dead-lettering");
                // Nack without requeue → broker routes to DLX/DLQ.
                let _ = channel
                    .basic_nack(tag, BasicNackOptions { requeue: false, ..Default::default() })
                    .await;
                return;
            }
        };

        let span = tracing::info_span!(
            "consumer.process",
            message.id   = %message.id,
            message.type = %message.message_type,
            retry_count  = message.retry_count,
        );
        let _enter = span.enter();

        match registry.dispatch(&message).await {
            Ok(()) => {
                tracing::debug!(message.id = %message.id, "Message handled successfully");
                let _ = channel
                    .basic_ack(tag, BasicAckOptions::default())
                    .await;
            }
            Err(e) => {
                message.increment_retry();
                tracing::warn!(
                    message.id    = %message.id,
                    retry_count   = message.retry_count,
                    max_retries   = self.max_retries,
                    error         = %e,
                    "Message handler failed"
                );

                if message.should_retry(self.max_retries) {
                    // Ack the original delivery, then re-publish with updated
                    // retry_count so the envelope stays accurate.
                    let _ = channel
                        .basic_ack(tag, BasicAckOptions::default())
                        .await;
                    self.requeue_with_backoff(channel, &message).await;
                } else {
                    tracing::error!(
                        message.id = %message.id,
                        "Message exceeded max retries, sending to DLQ"
                    );
                    // Ack original, publish to DLQ directly.
                    let _ = channel
                        .basic_ack(tag, BasicAckOptions::default())
                        .await;
                    self.publish_to_dlq(channel, &message, &e.to_string()).await;
                }
            }
        }
    }

    /// Re-publish a message to the main queue with an exponential-backoff delay
    /// encoded as the AMQP `expiration` property (milliseconds as a string).
    ///
    /// The message is published to the default exchange targeting the queue
    /// directly so it bypasses the main exchange routing and lands back in the
    /// same queue after the TTL expires.
    async fn requeue_with_backoff(&self, channel: &lapin::Channel, message: &Message) {
        let delay_ms = backoff_delay_ms(message.retry_count);
        tracing::info!(
            message.id  = %message.id,
            delay_ms,
            retry_count = message.retry_count,
            "Requeueing with backoff"
        );

        let payload = match serde_json::to_vec(message) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialise message for requeue");
                return;
            }
        };

        let props = BasicProperties::default()
            .with_content_type("application/json".into())
            .with_message_id(message.id.to_string().into())
            .with_delivery_mode(2)
            .with_expiration(delay_ms.to_string().into());

        if let Err(e) = channel
            .basic_publish(
                "",                  // default exchange
                &self.queue_name,    // route directly to queue
                BasicPublishOptions::default(),
                &payload,
                props,
            )
            .await
        {
            tracing::error!(error = %e, "Failed to requeue message with backoff");
        }
    }

    /// Publish a dead-letter envelope to the DLQ.
    async fn publish_to_dlq(&self, channel: &lapin::Channel, message: &Message, error: &str) {
        let dlm = DeadLetterMessage {
            id: Uuid::new_v4(),
            original_message: message.clone(),
            error: error.to_string(),
            failed_at: chrono::Utc::now(),
        };

        let payload = match serde_json::to_vec(&dlm) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialise dead-letter message");
                return;
            }
        };

        let props = BasicProperties::default()
            .with_content_type("application/json".into())
            .with_delivery_mode(2);

        if let Err(e) = channel
            .basic_publish(
                "",
                &self.dlq_name,
                BasicPublishOptions::default(),
                &payload,
                props,
            )
            .await
        {
            tracing::error!(
                error = %e,
                dlq   = %self.dlq_name,
                "Failed to publish to DLQ"
            );
        } else {
            tracing::error!(
                message.id = %message.id,
                dlq        = %self.dlq_name,
                "Message dead-lettered"
            );
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Exponential backoff delay in milliseconds for retry attempt `n`.
///
/// Sequence: 1 s, 2 s, 4 s, 8 s … capped at 60 s.
fn backoff_delay_ms(retry_count: u32) -> u64 {
    let base: u64 = 1_000;
    let max: u64 = 60_000;
    let delay = base.saturating_mul(2u64.saturating_pow(retry_count.saturating_sub(1)));
    delay.min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_sequence() {
        assert_eq!(backoff_delay_ms(1), 1_000);
        assert_eq!(backoff_delay_ms(2), 2_000);
        assert_eq!(backoff_delay_ms(3), 4_000);
        assert_eq!(backoff_delay_ms(4), 8_000);
        assert_eq!(backoff_delay_ms(5), 16_000);
        // Capped at 60 s
        assert_eq!(backoff_delay_ms(10), 60_000);
        assert_eq!(backoff_delay_ms(100), 60_000);
    }

    #[test]
    fn message_retry_logic() {
        let mut msg = Message::new("test", serde_json::Value::Null);
        assert!(msg.should_retry(3));
        msg.increment_retry();
        msg.increment_retry();
        msg.increment_retry();
        assert!(!msg.should_retry(3));
    }
}
