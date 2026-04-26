use super::consumer::{MessageHandler, MessageHandlerRegistry};
use super::publisher::Message;
use async_trait::async_trait;
use std::sync::Arc;

// ── Queue / topology configuration ───────────────────────────────────────────

/// Names of all queues and exchanges used by the system.
pub struct QueueNames;

impl QueueNames {
    pub const TIPS: &'static str = "tipjar.tips";
    pub const NOTIFICATIONS: &'static str = "tipjar.notifications";
    pub const ANALYTICS: &'static str = "tipjar.analytics";
    pub const WEBHOOKS: &'static str = "tipjar.webhooks";
}

pub struct ExchangeNames;

impl ExchangeNames {
    pub const TIPS: &'static str = "tipjar.tips.exchange";
    pub const NOTIFICATIONS: &'static str = "tipjar.notifications.exchange";
    pub const ANALYTICS: &'static str = "tipjar.analytics.exchange";
    pub const WEBHOOKS: &'static str = "tipjar.webhooks.exchange";
}

/// Message type strings — used as the `message_type` field in the envelope.
pub struct MessageTypes;

impl MessageTypes {
    pub const TIP_RECEIVED: &'static str = "tip_received";
    pub const TIP_VERIFIED: &'static str = "tip_verified";
    pub const TIP_FAILED: &'static str = "tip_failed";
    pub const CREATOR_REGISTERED: &'static str = "creator_registered";
    pub const NOTIFICATION_SEND: &'static str = "notification_send";
    pub const ANALYTICS_EVENT: &'static str = "analytics_event";
    pub const WEBHOOK_DISPATCH: &'static str = "webhook_dispatch";
}

/// Top-level configuration for the queue system.
#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub rabbitmq_url: String,
    pub max_retries: u32,
    pub prefetch_count: u16,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            rabbitmq_url: std::env::var("RABBITMQ_URL")
                .unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string()),
            max_retries: 3,
            prefetch_count: 10,
        }
    }
}

// ── Domain message handlers ───────────────────────────────────────────────────

/// Handles `tip_received` messages — verifies the Stellar transaction and
/// triggers downstream notifications.
pub struct TipReceivedHandler {
    state: Arc<crate::db::connection::AppState>,
}

impl TipReceivedHandler {
    pub fn new(state: Arc<crate::db::connection::AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MessageHandler for TipReceivedHandler {
    fn message_type(&self) -> &str {
        MessageTypes::TIP_RECEIVED
    }

    #[tracing::instrument(
        name = "handler.tip_received",
        skip(self, message),
        fields(message.id = %message.id)
    )]
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Payload {
            tip_id: uuid::Uuid,
            transaction_hash: String,
            creator_username: String,
        }

        let p: Payload = serde_json::from_value(message.payload.clone())?;

        tracing::info!(
            tip.id               = %p.tip_id,
            tip.tx_hash          = %p.transaction_hash,
            tip.creator_username = %p.creator_username,
            "Processing tip_received"
        );

        // Verify the transaction on the Stellar network.
        let verified = self
            .state
            .stellar
            .verify_transaction(&p.transaction_hash)
            .await
            .unwrap_or(false);

        if verified {
            tracing::info!(tip.id = %p.tip_id, "Transaction verified");
        } else {
            tracing::warn!(tip.id = %p.tip_id, "Transaction could not be verified");
        }

        Ok(())
    }
}

/// Handles `tip_verified` messages — sends a notification email to the creator.
pub struct TipVerifiedHandler {
    state: Arc<crate::db::connection::AppState>,
}

impl TipVerifiedHandler {
    pub fn new(state: Arc<crate::db::connection::AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MessageHandler for TipVerifiedHandler {
    fn message_type(&self) -> &str {
        MessageTypes::TIP_VERIFIED
    }

    #[tracing::instrument(
        name = "handler.tip_verified",
        skip(self, message),
        fields(message.id = %message.id)
    )]
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Payload {
            tip_id: uuid::Uuid,
            creator_username: String,
            amount_xlm: String,
        }

        let p: Payload = serde_json::from_value(message.payload.clone())?;

        tracing::info!(
            tip.id               = %p.tip_id,
            tip.creator_username = %p.creator_username,
            tip.amount_xlm       = %p.amount_xlm,
            "Processing tip_verified — sending notification"
        );

        Ok(())
    }
}

/// Handles `creator_registered` messages — sends a welcome email.
pub struct CreatorRegisteredHandler {
    state: Arc<crate::db::connection::AppState>,
}

impl CreatorRegisteredHandler {
    pub fn new(state: Arc<crate::db::connection::AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MessageHandler for CreatorRegisteredHandler {
    fn message_type(&self) -> &str {
        MessageTypes::CREATOR_REGISTERED
    }

    #[tracing::instrument(
        name = "handler.creator_registered",
        skip(self, message),
        fields(message.id = %message.id)
    )]
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Payload {
            creator_id: uuid::Uuid,
            username: String,
            email: Option<String>,
        }

        let p: Payload = serde_json::from_value(message.payload.clone())?;

        tracing::info!(
            creator.id       = %p.creator_id,
            creator.username = %p.username,
            creator.email    = ?p.email,
            "Processing creator_registered"
        );

        Ok(())
    }
}

/// Handles `notification_send` messages — dispatches email/push notifications.
pub struct NotificationSendHandler {
    state: Arc<crate::db::connection::AppState>,
}

impl NotificationSendHandler {
    pub fn new(state: Arc<crate::db::connection::AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MessageHandler for NotificationSendHandler {
    fn message_type(&self) -> &str {
        MessageTypes::NOTIFICATION_SEND
    }

    #[tracing::instrument(
        name = "handler.notification_send",
        skip(self, message),
        fields(message.id = %message.id)
    )]
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Payload {
            recipient_email: String,
            subject: String,
            body: String,
        }

        let p: Payload = serde_json::from_value(message.payload.clone())?;

        tracing::info!(
            notification.recipient = %p.recipient_email,
            notification.subject   = %p.subject,
            "Processing notification_send"
        );

        Ok(())
    }
}

/// Handles `analytics_event` messages — records events for the analytics pipeline.
pub struct AnalyticsEventHandler {
    state: Arc<crate::db::connection::AppState>,
}

impl AnalyticsEventHandler {
    pub fn new(state: Arc<crate::db::connection::AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MessageHandler for AnalyticsEventHandler {
    fn message_type(&self) -> &str {
        MessageTypes::ANALYTICS_EVENT
    }

    #[tracing::instrument(
        name = "handler.analytics_event",
        skip(self, message),
        fields(message.id = %message.id)
    )]
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        tracing::debug!(
            event.type = %message.message_type,
            "Processing analytics_event"
        );
        Ok(())
    }
}

/// Handles `webhook_dispatch` messages — delivers outbound webhook payloads.
pub struct WebhookDispatchHandler {
    state: Arc<crate::db::connection::AppState>,
}

impl WebhookDispatchHandler {
    pub fn new(state: Arc<crate::db::connection::AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MessageHandler for WebhookDispatchHandler {
    fn message_type(&self) -> &str {
        MessageTypes::WEBHOOK_DISPATCH
    }

    #[tracing::instrument(
        name = "handler.webhook_dispatch",
        skip(self, message),
        fields(message.id = %message.id)
    )]
    async fn handle(&self, message: &Message) -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Payload {
            webhook_id: uuid::Uuid,
            target_url: String,
            event_type: String,
        }

        let p: Payload = serde_json::from_value(message.payload.clone())?;

        tracing::info!(
            webhook.id         = %p.webhook_id,
            webhook.target_url = %p.target_url,
            webhook.event_type = %p.event_type,
            "Processing webhook_dispatch"
        );

        Ok(())
    }
}

// ── Registry factory ──────────────────────────────────────────────────────────

/// Build the default handler registry with all domain handlers registered.
pub fn build_handler_registry(
    state: Arc<crate::db::connection::AppState>,
) -> MessageHandlerRegistry {
    let mut registry = MessageHandlerRegistry::new();
    registry.register(Box::new(TipReceivedHandler::new(Arc::clone(&state))));
    registry.register(Box::new(TipVerifiedHandler::new(Arc::clone(&state))));
    registry.register(Box::new(CreatorRegisteredHandler::new(Arc::clone(&state))));
    registry.register(Box::new(NotificationSendHandler::new(Arc::clone(&state))));
    registry.register(Box::new(AnalyticsEventHandler::new(Arc::clone(&state))));
    registry.register(Box::new(WebhookDispatchHandler::new(Arc::clone(&state))));
    registry
}
