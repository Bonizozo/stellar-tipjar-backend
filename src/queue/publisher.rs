use super::connection::RabbitMQConnection;
use lapin::{
    options::BasicPublishOptions,
    BasicProperties,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// ── Message envelope ─────────────────────────────────────────────────────────

/// Envelope wrapping every message published to RabbitMQ.
///
/// The envelope carries routing metadata alongside the domain payload so
/// consumers can dispatch without deserialising the inner payload first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message identifier (used for idempotency checks).
    pub id: Uuid,
    /// Logical message type — consumers use this to route to the right handler.
    pub message_type: String,
    /// Domain payload; structure depends on `message_type`.
    pub payload: serde_json::Value,
    /// Wall-clock time the message was first created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// How many times delivery has been attempted (incremented by the consumer).
    pub retry_count: u32,
}

impl Message {
    pub fn new(message_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            message_type: message_type.into(),
            payload,
            created_at: chrono::Utc::now(),
            retry_count: 0,
        }
    }

    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    pub fn should_retry(&self, max_retries: u32) -> bool {
        self.retry_count < max_retries
    }
}

/// A message that has been permanently moved to the dead-letter queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterMessage {
    pub id: Uuid,
    pub original_message: Message,
    /// Human-readable reason the message was dead-lettered.
    pub error: String,
    pub failed_at: chrono::DateTime<chrono::Utc>,
}

// ── Publisher ─────────────────────────────────────────────────────────────────

/// Publishes messages to a RabbitMQ exchange.
///
/// Each `MessagePublisher` owns its own AMQP channel so concurrent publishers
/// don't contend on a shared channel.
pub struct MessagePublisher {
    connection: Arc<RabbitMQConnection>,
    exchange_name: String,
    routing_key: String,
}

impl MessagePublisher {
    pub async fn new(
        connection: Arc<RabbitMQConnection>,
        exchange_name: impl Into<String>,
        routing_key: impl Into<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            connection,
            exchange_name: exchange_name.into(),
            routing_key: routing_key.into(),
        })
    }

    /// Publish a single message.
    ///
    /// Sets AMQP `content_type`, `message_id`, and `delivery_mode = 2`
    /// (persistent) so messages survive broker restarts.
    ///
    /// Each publish creates a short-lived channel.  For high-throughput
    /// scenarios consider pooling channels; for this workload the overhead
    /// is acceptable and keeps the API simple.
    #[tracing::instrument(
        name = "publisher.publish",
        skip(self, message),
        fields(
            message.id        = %message.id,
            message.type      = %message.message_type,
            exchange          = %self.exchange_name,
            routing_key       = %self.routing_key,
        )
    )]
    pub async fn publish(&self, message: &Message) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(message)?;
        let channel = self.connection.create_channel().await?;

        let props = BasicProperties::default()
            .with_content_type("application/json".into())
            .with_message_id(message.id.to_string().into())
            .with_delivery_mode(2); // persistent

        // `basic_publish` returns a `PublisherConfirm` future.  We drop it
        // here (fire-and-forget) because the channel is not in confirm mode.
        // To enable at-least-once delivery guarantees, call
        // `channel.confirm_select(...)` before publishing and then `.await`
        // the returned future.
        let _confirm = channel
            .basic_publish(
                &self.exchange_name,
                &self.routing_key,
                BasicPublishOptions::default(),
                &payload,
                props,
            )
            .await?;

        tracing::info!(
            message.id = %message.id,
            message.type = %message.message_type,
            "Message published"
        );
        Ok(())
    }

    /// Publish a batch of messages.  Each message is published individually;
    /// failures are collected and returned as a combined error.
    pub async fn publish_batch(&self, messages: &[Message]) -> anyhow::Result<()> {
        let mut errors: Vec<String> = Vec::new();

        for msg in messages {
            if let Err(e) = self.publish(msg).await {
                errors.push(format!("{}: {}", msg.id, e));
            }
        }

        if errors.is_empty() {
            tracing::info!(count = messages.len(), "Batch published successfully");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Batch publish had {} error(s): {}",
                errors.len(),
                errors.join("; ")
            ))
        }
    }
}
