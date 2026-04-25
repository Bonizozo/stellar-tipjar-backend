use super::publisher::{Message, MessageConsumer, MessagePublisher};
use async_trait::async_trait;
use std::sync::Arc;

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

pub struct QueueConfig {
    pub rabbitmq_url: String,
    pub queue_name: String,
    pub exchange_name: String,
    pub routing_key: String,
    pub max_retries: u32,
    pub prefetch_count: u16,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            rabbitmq_url: std::env::var("RABBITMQ_URL")
                .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672".to_string()),
            queue_name: "stellar-tipjar".to_string(),
            exchange_name: "stellar-tipjar-exchange".to_string(),
            routing_key: "tipjar.*".to_string(),
            max_retries: 3,
            prefetch_count: 10,
        }
    }
}

pub async fn initialize_queue_system(
    config: QueueConfig,
) -> anyhow::Result<(Arc<MessagePublisher>, Arc<MessageConsumer>)> {
    use super::connection::RabbitMQConnection;

    tracing::info!("Initializing RabbitMQ queue system");

    let connection = Arc::new(RabbitMQConnection::connect(&config.rabbitmq_url).await?);

    connection
        .setup_queue_with_dlq(&config.queue_name, &config.exchange_name, &config.routing_key)
        .await?;

    let publisher = Arc::new(MessagePublisher::new(
        connection.clone(),
        config.queue_name.clone(),
        config.exchange_name,
        config.routing_key,
        config.max_retries,
    ));

    let consumer = Arc::new(MessageConsumer::new(
        connection,
        config.queue_name,
        config.max_retries,
    ));

    tracing::info!("Queue system initialized successfully");
    Ok((publisher, consumer))
}

pub fn create_handler_registry() -> Arc<MessageHandlerRegistry> {
    let mut registry = MessageHandlerRegistry::new();
    registry.register(Box::new(TipEventHandler));
    registry.register(Box::new(CreatorNotificationHandler));
    registry.register(Box::new(AnalyticsEventHandler));
    Arc::new(registry)
}

pub async fn start_consumer_worker(
    consumer: Arc<MessageConsumer>,
    registry: Arc<MessageHandlerRegistry>,
) {
    loop {
        match consumer.consume().await {
            Ok(Some(message)) => {
                match registry.handle(&message).await {
                    Ok(_) => {
                        tracing::debug!("Message {} processed successfully", message.id);
                    }
                    Err(e) => {
                        tracing::error!("Error handling message {}: {}", message.id, e);
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
