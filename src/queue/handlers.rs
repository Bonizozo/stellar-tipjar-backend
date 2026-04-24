use super::publisher::{Message, MessageConsumer, MessagePublisher};
use super::consumer::{MessageHandlerRegistry, TipEventHandler, CreatorNotificationHandler, AnalyticsEventHandler};

pub struct QueueConfig {
    pub rabbitmq_url: String,
    pub queue_name: String,
    pub max_retries: u32,
    pub prefetch_count: u16,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            rabbitmq_url: std::env::var("RABBITMQ_URL")
                .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672".to_string()),
            queue_name: "stellar-tipjar".to_string(),
            max_retries: 3,
            prefetch_count: 10,
        }
    }
}

pub async fn initialize_queue_system(config: QueueConfig) -> anyhow::Result<(MessagePublisher, MessageConsumer)> {
    tracing::info!("Initializing RabbitMQ queue system");
    
    let publisher = MessagePublisher::new(config.queue_name.clone(), config.max_retries);
    let consumer = MessageConsumer::new(config.queue_name, config.max_retries);

    tracing::info!("Queue system initialized successfully");
    Ok((publisher, consumer))
}

pub fn create_handler_registry() -> MessageHandlerRegistry {
    let mut registry = MessageHandlerRegistry::new();
    registry.register(Box::new(TipEventHandler));
    registry.register(Box::new(CreatorNotificationHandler));
    registry.register(Box::new(AnalyticsEventHandler));
    registry
}
