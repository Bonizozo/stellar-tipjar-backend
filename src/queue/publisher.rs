use super::connection::RabbitMQConnection;
use lapin::options::BasicPublishOptions;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
    connection: Arc<RabbitMQConnection>,
    queue_name: String,
    exchange_name: String,
    routing_key: String,
    max_retries: u32,
}

impl MessagePublisher {
    pub fn new(
        connection: Arc<RabbitMQConnection>,
        queue_name: String,
        exchange_name: String,
        routing_key: String,
        max_retries: u32,
    ) -> Self {
        Self {
            connection,
            queue_name,
            exchange_name,
            routing_key,
            max_retries,
        }
    }

    pub async fn publish(&self, message: Message) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(&message)?;
        let channel = self.connection.channel();

        channel
            .basic_publish(
                &self.exchange_name,
                &self.routing_key,
                BasicPublishOptions::default(),
                &payload,
                Default::default(),
            )
            .await?;

        tracing::info!(
            "Published message {} to exchange {} with routing key {}",
            message.id,
            self.exchange_name,
            self.routing_key
        );
        Ok(())
    }

    pub async fn publish_batch(&self, messages: Vec<Message>) -> anyhow::Result<()> {
        for message in messages {
            self.publish(message).await?;
        }
        tracing::info!(
            "Published {} messages to queue {}",
            messages.len(),
            self.queue_name
        );
        Ok(())
    }
}

pub struct MessageConsumer {
    connection: Arc<RabbitMQConnection>,
    queue_name: String,
    max_retries: u32,
    dead_letter_queue: String,
}

impl MessageConsumer {
    pub fn new(
        connection: Arc<RabbitMQConnection>,
        queue_name: String,
        max_retries: u32,
    ) -> Self {
        Self {
            connection,
            queue_name: queue_name.clone(),
            max_retries,
            dead_letter_queue: format!("{}.dlq", queue_name),
        }
    }

    pub async fn consume(&self) -> anyhow::Result<Option<Message>> {
        let channel = self.connection.channel();
        let delivery = channel
            .basic_consume(
                &self.queue_name,
                "",
                lapin::options::BasicConsumeOptions {
                    no_ack: false,
                    ..Default::default()
                },
                Default::default(),
            )
            .await?;

        use futures_lite::stream::StreamExt;
        if let Some(delivery) = delivery.next().await {
            let delivery = delivery?;
            let message: Message = serde_json::from_slice(&delivery.data)?;
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    pub async fn acknowledge(&self, delivery_tag: u64) -> anyhow::Result<()> {
        let channel = self.connection.channel();
        channel
            .basic_ack(delivery_tag, lapin::options::BasicAckOptions::default())
            .await?;
        tracing::debug!("Acknowledged message with tag {}", delivery_tag);
        Ok(())
    }

    pub async fn nack(&self, message: &mut Message, delivery_tag: u64) -> anyhow::Result<()> {
        message.increment_retry();
        let channel = self.connection.channel();

        if message.should_retry(self.max_retries) {
            tracing::warn!(
                "Retrying message {} (attempt {})",
                message.id,
                message.retry_count
            );
            channel
                .basic_nack(
                    delivery_tag,
                    lapin::options::BasicNackOptions {
                        requeue: true,
                        ..Default::default()
                    },
                )
                .await?;
            Ok(())
        } else {
            tracing::error!("Message {} exceeded max retries, sending to DLQ", message.id);
            self.send_to_dlq(message).await?;
            channel
                .basic_ack(delivery_tag, lapin::options::BasicAckOptions::default())
                .await?;
            Ok(())
        }
    }

    async fn send_to_dlq(&self, message: &Message) -> anyhow::Result<()> {
        let dlq_message = DeadLetterMessage {
            id: Uuid::new_v4(),
            original_message: message.clone(),
            error: format!("Exceeded max retries: {}", self.max_retries),
            failed_at: chrono::Utc::now(),
        };

        let payload = serde_json::to_vec(&dlq_message)?;
        let channel = self.connection.channel();

        channel
            .basic_publish(
                "",
                &self.dead_letter_queue,
                BasicPublishOptions::default(),
                &payload,
                Default::default(),
            )
            .await?;

        tracing::error!(
            "Sent message {} to dead letter queue {}",
            message.id,
            self.dead_letter_queue
        );
        Ok(())
    }
}
