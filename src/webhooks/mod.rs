pub mod sender;
pub mod signature;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Webhook {
    pub id: Uuid,
    pub url: String,
    pub secret: String,
    pub enabled: bool,
    pub events: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub timestamp: DateTime<Utc>,
}

/// Triggers registered webhooks for a given event.
/// Runs asynchronously in a background task to avoid blocking API responses.
pub async fn trigger_webhooks(
    pool: sqlx::PgPool, 
    event_type: &str, 
    payload: Value
) {
    let pool_clone = pool.clone();
    let event_name = event_type.to_string();
    let payload_clone = payload.clone();

    tokio::spawn(async move {
        let webhooks = match sqlx::query_as::<_, Webhook>(
            "SELECT * FROM webhooks WHERE enabled = TRUE AND $1 = ANY(events)"
        )
        .bind(&event_name)
        .fetch_all(&pool_clone)
        .await {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to fetch webhooks for event {}: {}", event_name, e);
                return;
            }
        };

        tracing::info!("Found {} webhooks for event {}", webhooks.len(), event_name);

        for webhook in webhooks {
            let event = WebhookEvent {
                id: Uuid::new_v4(),
                event_type: event_name.clone(),
                payload: payload_clone.clone(),
                timestamp: Utc::now(),
            };
            
            let url = webhook.url.clone();
            let secret = webhook.secret.clone();
            let event_value = serde_json::to_value(event).unwrap();

            tokio::spawn(async move {
                if let Err(e) = sender::send_webhook_with_retry(url, secret, event_value).await {
                    tracing::error!("Final failure sending webhook to {}: {}", webhook.id, e);
                }
            });
        }
    });
}
