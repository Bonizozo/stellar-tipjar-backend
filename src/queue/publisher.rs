use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub message_type: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterMessage {
    pub id: Uuid,
    pub original_message: Message,
    pub error: String,
    pub failed_at: chrono::DateTime<chrono::Utc>,
}

impl Message {
    pub fn new(message_type: String, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            message_type,
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

pub struct MessagePublisher {
    queue_name: String,
    max_retries: u32,
}

impl MessagePublisher {
    pub fn new(queue_name: String, max_retries: u32) -> Self {
        Self {
            queue_name,
            max_retries,
        }
    }

    pub async fn publish(&self, message: Message) -> anyhow::Result<()> {
        tracing::info!(
            "Publishing message {} to queue {}",
            message.id,
            self.queue_name
        );
        Ok(())
    }

    pub async fn publish_batch(&self, messages: Vec<Message>) -> anyhow::Result<()> {
        tracing::info!(
            "Publishing {} messages to queue {}",
            messages.len(),
            self.queue_name
        );
        for message in messages {
            self.publish(message).await?;
        }
        Ok(())
    }
}

pub struct MessageConsumer {
    queue_name: String,
    max_retries: u32,
    dead_letter_queue: String,
}

impl MessageConsumer {
    pub fn new(queue_name: String, max_retries: u32) -> Self {
        Self {
            queue_name: queue_name.clone(),
            max_retries,
            dead_letter_queue: format!("{}.dlq", queue_name),
        }
    }

    pub async fn consume(&self) -> anyhow::Result<Option<Message>> {
        tracing::debug!("Consuming from queue {}", self.queue_name);
        Ok(None)
    }

    pub async fn acknowledge(&self, message_id: Uuid) -> anyhow::Result<()> {
        tracing::debug!("Acknowledging message {}", message_id);
        Ok(())
    }

    pub async fn nack(&self, message: &mut Message) -> anyhow::Result<()> {
        message.increment_retry();
        if message.should_retry(self.max_retries) {
            tracing::warn!(
                "Retrying message {} (attempt {})",
                message.id,
                message.retry_count
            );
            Ok(())
        } else {
            tracing::error!("Message {} exceeded max retries, sending to DLQ", message.id);
            self.send_to_dlq(message).await
        }
    }

    async fn send_to_dlq(&self, message: &Message) -> anyhow::Result<()> {
        tracing::error!(
            "Sending message {} to dead letter queue {}",
            message.id,
            self.dead_letter_queue
        );
        Ok(())
    }
}
