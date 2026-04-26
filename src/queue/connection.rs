use lapin::{
    options::*, types::FieldTable, Channel, Connection, ConnectionProperties, Result as LapinResult,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// A live RabbitMQ connection plus a dedicated channel.
///
/// The channel is used for topology declarations (exchanges, queues, bindings).
/// Publishers and consumers each create their own channels via `create_channel()`
/// so they don't share state or contend on the same channel mutex.
#[derive(Clone)]
pub struct RabbitMQConnection {
    connection: Arc<Connection>,
}

impl RabbitMQConnection {
    /// Connect to RabbitMQ with exponential-backoff retry.
    ///
    /// Retries up to `max_retries` times with delays 1 s, 2 s, 4 s … capped at 30 s.
    pub async fn connect(uri: &str) -> anyhow::Result<Self> {
        let mut delay = Duration::from_secs(1);
        let max_retries = 5u32;

        for attempt in 1..=max_retries {
            match Connection::connect(uri, ConnectionProperties::default()).await {
                Ok(conn) => {
                    info!("Connected to RabbitMQ (attempt {})", attempt);
                    return Ok(Self {
                        connection: Arc::new(conn),
                    });
                }
                Err(e) if attempt < max_retries => {
                    tracing::warn!(
                        attempt,
                        delay_secs = delay.as_secs(),
                        error = %e,
                        "RabbitMQ connection failed, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(30));
                }
                Err(e) => return Err(e.into()),
            }
        }
        unreachable!()
    }

    /// Create a fresh channel.  Each publisher / consumer should own its own.
    pub async fn create_channel(&self) -> LapinResult<Channel> {
        self.connection.create_channel().await
    }

    /// Declare a durable direct exchange.
    pub async fn declare_exchange(
        &self,
        channel: &Channel,
        exchange_name: &str,
        exchange_type: lapin::ExchangeKind,
    ) -> LapinResult<()> {
        channel
            .exchange_declare(
                exchange_name,
                exchange_type,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;
        Ok(())
    }

    /// Declare a durable queue with optional extra arguments (e.g. DLX config).
    pub async fn declare_queue(
        &self,
        channel: &Channel,
        queue_name: &str,
        args: FieldTable,
    ) -> LapinResult<lapin::queue::QueueDeclareOk> {
        channel
            .queue_declare(
                queue_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                args,
            )
            .await
    }

    /// Bind a queue to an exchange with a routing key.
    pub async fn bind_queue(
        &self,
        channel: &Channel,
        queue_name: &str,
        exchange_name: &str,
        routing_key: &str,
    ) -> LapinResult<()> {
        channel
            .queue_bind(
                queue_name,
                exchange_name,
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;
        Ok(())
    }

    /// Declare the full topology for a queue:
    ///
    /// ```text
    /// [main exchange] --routing_key--> [main queue]
    ///                                       |  (on rejection / TTL expiry)
    ///                                       v
    ///                              [DLX exchange] --> [DLQ]
    /// ```
    ///
    /// The main queue is configured with `x-dead-letter-exchange` pointing at
    /// the DLX so that messages rejected without requeue (or that expire) are
    /// automatically routed to the DLQ.
    pub async fn setup_topology(
        &self,
        channel: &Channel,
        queue_name: &str,
        exchange_name: &str,
        routing_key: &str,
    ) -> anyhow::Result<()> {
        let dlq_name = format!("{}.dlq", queue_name);
        let dlx_name = format!("{}.dlx", exchange_name);

        // 1. Dead-letter exchange (direct)
        self.declare_exchange(channel, &dlx_name, lapin::ExchangeKind::Direct)
            .await?;

        // 2. Dead-letter queue (plain durable, no extra args)
        self.declare_queue(channel, &dlq_name, FieldTable::default())
            .await?;

        // 3. Bind DLQ → DLX using the main queue name as routing key
        self.bind_queue(channel, &dlq_name, &dlx_name, queue_name)
            .await?;

        // 4. Main exchange
        self.declare_exchange(channel, exchange_name, lapin::ExchangeKind::Direct)
            .await?;

        // 5. Main queue — points dead letters at the DLX
        let mut queue_args = FieldTable::default();
        queue_args.insert(
            "x-dead-letter-exchange".into(),
            lapin::types::AMQPValue::LongString(dlx_name.into()),
        );
        queue_args.insert(
            "x-dead-letter-routing-key".into(),
            lapin::types::AMQPValue::LongString(queue_name.into()),
        );
        self.declare_queue(channel, queue_name, queue_args).await?;

        // 6. Bind main queue → main exchange
        self.bind_queue(channel, queue_name, exchange_name, routing_key)
            .await?;

        info!(
            queue = queue_name,
            dlq = dlq_name,
            exchange = exchange_name,
            "Queue topology configured"
        );
        Ok(())
    }

    /// Gracefully close the underlying AMQP connection.
    pub async fn close(&self) -> LapinResult<()> {
        self.connection.close(200, "Normal shutdown").await
    }
}
