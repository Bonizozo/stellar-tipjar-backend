use super::publisher::{Message, MessageConsumer};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait MessageHandler: Send + Sync {
    async fn handle(&self, message: &Message) -> anyhow::Result<()>;
    fn message_type(&self) -> &str;
}

pub struct TipEventHandler;

#[async_trait]
impl MessageHandler for TipEventHandler {
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        tracing::info!("Handling tip event: {}", message.id);
        Ok(())
    }

    fn message_type(&self) -> &str {
        "tip_received"
    }
}

pub struct CreatorNotificationHandler;

#[async_trait]
impl MessageHandler for CreatorNotificationHandler {
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        tracing::info!("Handling creator notification: {}", message.id);
        Ok(())
    }

    fn message_type(&self) -> &str {
        "creator_notification"
    }
}

pub struct AnalyticsEventHandler;

#[async_trait]
impl MessageHandler for AnalyticsEventHandler {
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        tracing::info!("Handling analytics event: {}", message.id);
        Ok(())
    }

    fn message_type(&self) -> &str {
        "analytics_event"
    }
}

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

    pub async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        if let Some(handler) = self.handlers.get(&message.message_type) {
            handler.handle(message).await
        } else {
            tracing::warn!("No handler found for message type: {}", message.message_type);
            Ok(())
        }
    }
}

pub async fn start_consumer_worker(
    consumer: MessageConsumer,
    registry: MessageHandlerRegistry,
) {
    loop {
        match consumer.consume().await {
            Ok(Some(mut message)) => {
                match registry.handle(&message).await {
                    Ok(_) => {
                        let _ = consumer.acknowledge(message.id).await;
                    }
                    Err(e) => {
                        tracing::error!("Error handling message {}: {}", message.id, e);
                        let _ = consumer.nack(&mut message).await;
                    }
                }
            }
            Ok(None) => {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
            Err(e) => {
                tracing::error!("Error consuming message: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}
