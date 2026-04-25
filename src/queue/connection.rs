use lapin::{
    options::*, types::FieldTable, Channel, Connection, ConnectionProperties, Result as LapinResult,
};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Clone)]
pub struct RabbitMQConnection {
    connection: Arc<Connection>,
    channel: Arc<Channel>,
}

impl RabbitMQConnection {
    pub async fn connect(uri: &str) -> anyhow::Result<Self> {
        info!("Connecting to RabbitMQ at {}", uri);

        let connection = Connection::connect(uri, ConnectionProperties::default()).await?;
        info!("Connected to RabbitMQ");

        let channel = connection.create_channel().await?;
        info!("Created RabbitMQ channel");

        Ok(Self {
            connection: Arc::new(connection),
            channel: Arc::new(channel),
        })
    }

    pub fn channel(&self) -> Arc<Channel> {
        Arc::clone(&self.channel)
    }

    pub async fn declare_queue(
        &self,
        queue_name: &str,
        durable: bool,
    ) -> LapinResult<lapin::queue::QueueDeclareOk> {
        self.channel
            .queue_declare(
                queue_name,
                QueueDeclareOptions {
                    durable,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
    }

    pub async fn declare_exchange(
        &self,
        exchange_name: &str,
        exchange_type: lapin::ExchangeKind,
        durable: bool,
    ) -> LapinResult<lapin::exchange::ExchangeDeclareOk> {
        self.channel
            .exchange_declare(
                exchange_name,
                exchange_type,
                ExchangeDeclareOptions {
                    durable,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
    }

    pub async fn bind_queue(
        &self,
        queue_name: &str,
        exchange_name: &str,
        routing_key: &str,
    ) -> LapinResult<lapin::queue::QueueBindOk> {
        self.channel
            .queue_bind(
                queue_name,
                exchange_name,
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
    }

    pub async fn declare_dlq(&self, queue_name: &str) -> LapinResult<lapin::queue::QueueDeclareOk> {
        let dlq_name = format!("{}.dlq", queue_name);
        self.channel
            .queue_declare(
                &dlq_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
    }

    pub async fn setup_queue_with_dlq(
        &self,
        queue_name: &str,
        exchange_name: &str,
        routing_key: &str,
    ) -> anyhow::Result<()> {
        let dlq_name = format!("{}.dlq", queue_name);
        let dlx_name = format!("{}.dlx", queue_name);

        // Declare DLX (Dead Letter Exchange)
        self.declare_exchange(&dlx_name, lapin::ExchangeKind::Direct, true)
            .await?;

        // Declare DLQ
        let mut dlq_args = FieldTable::default();
        self.channel
            .queue_declare(
                &dlq_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                dlq_args,
            )
            .await?;

        // Bind DLQ to DLX
        self.channel
            .queue_bind(
                &dlq_name,
                &dlx_name,
                queue_name,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Declare main exchange
        self.declare_exchange(exchange_name, lapin::ExchangeKind::Direct, true)
            .await?;

        // Declare main queue with DLX configuration
        let mut queue_args = FieldTable::default();
        queue_args.insert("x-dead-letter-exchange".into(), dlx_name.into());
        queue_args.insert("x-dead-letter-routing-key".into(), queue_name.into());

        self.channel
            .queue_declare(
                queue_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                queue_args,
            )
            .await?;

        // Bind main queue to exchange
        self.channel
            .queue_bind(
                queue_name,
                exchange_name,
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        info!(
            "Queue {} with DLQ {} configured successfully",
            queue_name, dlq_name
        );
        Ok(())
    }

    pub async fn close(&self) -> LapinResult<()> {
        self.connection.close(200, "Normal shutdown").await
    }
}
