pub mod connection;
pub mod consumer;
pub mod handlers;
pub mod publisher;
pub mod system;

// Flat re-exports for the most commonly used types.
pub use connection::RabbitMQConnection;
pub use consumer::{MessageConsumer, MessageHandler, MessageHandlerRegistry};
pub use handlers::{
    build_handler_registry, ExchangeNames, MessageTypes, QueueConfig, QueueNames,
};
pub use publisher::{DeadLetterMessage, Message, MessagePublisher};
pub use system::{try_start, QueueSystem};
