use super::{
    connection::RabbitMQConnection,
    consumer::{MessageConsumer, MessageHandlerRegistry},
    handlers::{build_handler_registry, ExchangeNames, QueueConfig, QueueNames},
    publisher::MessagePublisher,
};
use crate::db::connection::AppState;
use std::sync::Arc;
use tokio::sync::broadcast;

/// A running queue system: publisher handle + shutdown sender.
pub struct QueueSystem {
    /// Publish messages to the tips exchange.
    pub tips_publisher: Arc<MessagePublisher>,
    /// Publish messages to the notifications exchange.
    pub notifications_publisher: Arc<MessagePublisher>,
    /// Publish messages to the analytics exchange.
    pub analytics_publisher: Arc<MessagePublisher>,
    /// Publish messages to the webhooks exchange.
    pub webhooks_publisher: Arc<MessagePublisher>,
    /// Send `()` to stop all consumer workers.
    shutdown_tx: broadcast::Sender<()>,
}

impl QueueSystem {
    /// Initialise the full queue system:
    ///
    /// 1. Connect to RabbitMQ.
    /// 2. Declare all exchanges, queues, and DLQ topologies.
    /// 3. Build publishers for each exchange.
    /// 4. Spawn consumer workers for each queue.
    ///
    /// Returns `None` when `RABBITMQ_URL` is not set, so the app starts
    /// cleanly in environments without a broker.
    pub async fn start(
        config: QueueConfig,
        state: Arc<AppState>,
    ) -> anyhow::Result<Self> {
        tracing::info!("Initialising RabbitMQ queue system");

        let conn = Arc::new(RabbitMQConnection::connect(&config.rabbitmq_url).await?);

        // ── Declare topology ──────────────────────────────────────────────────
        let topology_channel = conn.create_channel().await?;

        let queues = [
            (QueueNames::TIPS, ExchangeNames::TIPS, QueueNames::TIPS),
            (QueueNames::NOTIFICATIONS, ExchangeNames::NOTIFICATIONS, QueueNames::NOTIFICATIONS),
            (QueueNames::ANALYTICS, ExchangeNames::ANALYTICS, QueueNames::ANALYTICS),
            (QueueNames::WEBHOOKS, ExchangeNames::WEBHOOKS, QueueNames::WEBHOOKS),
        ];

        for (queue, exchange, routing_key) in &queues {
            conn.setup_topology(&topology_channel, queue, exchange, routing_key)
                .await?;
        }

        // ── Publishers ────────────────────────────────────────────────────────
        let tips_publisher = Arc::new(
            MessagePublisher::new(Arc::clone(&conn), ExchangeNames::TIPS, QueueNames::TIPS).await?,
        );
        let notifications_publisher = Arc::new(
            MessagePublisher::new(
                Arc::clone(&conn),
                ExchangeNames::NOTIFICATIONS,
                QueueNames::NOTIFICATIONS,
            )
            .await?,
        );
        let analytics_publisher = Arc::new(
            MessagePublisher::new(
                Arc::clone(&conn),
                ExchangeNames::ANALYTICS,
                QueueNames::ANALYTICS,
            )
            .await?,
        );
        let webhooks_publisher = Arc::new(
            MessagePublisher::new(
                Arc::clone(&conn),
                ExchangeNames::WEBHOOKS,
                QueueNames::WEBHOOKS,
            )
            .await?,
        );

        // ── Consumer workers ──────────────────────────────────────────────────
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let registry = Arc::new(build_handler_registry(Arc::clone(&state)));

        for (queue_name, _, _) in &queues {
            let consumer = MessageConsumer::new(
                Arc::clone(&conn),
                *queue_name,
                config.max_retries,
                config.prefetch_count,
            );
            let registry = Arc::clone(&registry);
            let shutdown_rx = shutdown_tx.subscribe();

            tokio::spawn(async move {
                consumer.run(registry, shutdown_rx).await;
            });
        }

        tracing::info!("RabbitMQ queue system started");

        Ok(Self {
            tips_publisher,
            notifications_publisher,
            analytics_publisher,
            webhooks_publisher,
            shutdown_tx,
        })
    }

    /// Signal all consumer workers to stop.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        tracing::info!("Queue system shutdown signal sent");
    }
}

/// Attempt to start the queue system.  Returns `None` and logs a warning when
/// `RABBITMQ_URL` is not configured or the broker is unreachable, so the rest
/// of the application continues to function without a message queue.
pub async fn try_start(state: Arc<AppState>) -> Option<Arc<QueueSystem>> {
    if std::env::var("RABBITMQ_URL").is_err() {
        tracing::info!("RABBITMQ_URL not set — message queue disabled");
        return None;
    }

    let config = QueueConfig::default();

    match QueueSystem::start(config, state).await {
        Ok(system) => {
            tracing::info!("Message queue system running");
            Some(Arc::new(system))
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to start message queue — continuing without it");
            None
        }
    }
}
