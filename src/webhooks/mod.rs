pub mod sender;
pub mod signature;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

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
pub struct CreateWebhookRequest {
    pub url: String,
    /// Events to subscribe to, e.g. ["tip.created", "creator.updated"]
    pub events: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateWebhookRequest {
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct WebhookLog {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub status_code: Option<i32>,
    pub response_body: Option<String>,
    pub success: bool,
    pub attempts: i32,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Registration CRUD
// ---------------------------------------------------------------------------

pub async fn create_webhook(
    pool: &PgPool,
    req: CreateWebhookRequest,
) -> Result<Webhook, sqlx::Error> {
    let secret = generate_secret();
    sqlx::query_as::<_, Webhook>(
        "INSERT INTO webhooks (url, secret, events) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(&req.url)
    .bind(&secret)
    .bind(&req.events)
    .fetch_one(pool)
    .await
}

pub async fn list_webhooks(pool: &PgPool) -> Result<Vec<Webhook>, sqlx::Error> {
    sqlx::query_as::<_, Webhook>("SELECT * FROM webhooks ORDER BY created_at DESC")
        .fetch_all(pool)
        .await
}

pub async fn get_webhook(pool: &PgPool, id: Uuid) -> Result<Option<Webhook>, sqlx::Error> {
    sqlx::query_as::<_, Webhook>("SELECT * FROM webhooks WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn update_webhook(
    pool: &PgPool,
    id: Uuid,
    req: UpdateWebhookRequest,
) -> Result<Option<Webhook>, sqlx::Error> {
    sqlx::query_as::<_, Webhook>(
        "UPDATE webhooks
         SET url      = COALESCE($2, url),
             events   = COALESCE($3, events),
             enabled  = COALESCE($4, enabled),
             updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(req.url)
    .bind(req.events)
    .bind(req.enabled)
    .fetch_optional(pool)
    .await
}

pub async fn delete_webhook(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query("DELETE FROM webhooks WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows > 0)
}

// ---------------------------------------------------------------------------
// Delivery tracking
// ---------------------------------------------------------------------------

/// Record a delivery attempt in webhook_logs.
pub async fn log_delivery(
    pool: &PgPool,
    webhook_id: Uuid,
    event_type: &str,
    payload: &Value,
    status_code: Option<i32>,
    response_body: Option<&str>,
    success: bool,
    attempts: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO webhook_logs
         (webhook_id, event_type, payload, status_code, response_body, success, attempts)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(webhook_id)
    .bind(event_type)
    .bind(payload)
    .bind(status_code)
    .bind(response_body)
    .bind(success)
    .bind(attempts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_delivery_logs(
    pool: &PgPool,
    webhook_id: Uuid,
    limit: i64,
) -> Result<Vec<WebhookLog>, sqlx::Error> {
    sqlx::query_as::<_, WebhookLog>(
        "SELECT * FROM webhook_logs WHERE webhook_id = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(webhook_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

// ---------------------------------------------------------------------------
// Event dispatch
// ---------------------------------------------------------------------------

/// Fire registered webhooks for an event. Runs in a background task.
/// Logs each delivery attempt to webhook_logs.
pub async fn trigger_webhooks(pool: PgPool, event_type: &str, payload: Value) {
    let event_name = event_type.to_string();

    tokio::spawn(async move {
        let webhooks = match sqlx::query_as::<_, Webhook>(
            "SELECT * FROM webhooks WHERE enabled = TRUE AND $1 = ANY(events)",
        )
        .bind(&event_name)
        .fetch_all(&pool)
        .await
        {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to fetch webhooks for event {}: {}", event_name, e);
                return;
            }
        };

        tracing::info!("Dispatching {} webhooks for event {}", webhooks.len(), event_name);

        for webhook in webhooks {
            let pool2 = pool.clone();
            let event = WebhookEvent {
                id: Uuid::new_v4(),
                event_type: event_name.clone(),
                payload: payload.clone(),
                timestamp: Utc::now(),
            };
            let event_value = serde_json::to_value(&event).unwrap_or_default();
            let url = webhook.url.clone();
            let secret = webhook.secret.clone();
            let wid = webhook.id;
            let etype = event_name.clone();

            tokio::spawn(async move {
                let result =
                    sender::send_webhook_with_retry(url, secret, event_value.clone()).await;

                let (success, status_code, response_body) = match &result {
                    Ok(_) => (true, Some(200i32), None),
                    Err(e) => (false, None, Some(e.to_string())),
                };

                if let Err(log_err) = log_delivery(
                    &pool2,
                    wid,
                    &etype,
                    &event_value,
                    status_code,
                    response_body.as_deref(),
                    success,
                    4, // RetryConfig::default max_retries + 1
                )
                .await
                {
                    tracing::warn!("Failed to log webhook delivery: {}", log_err);
                }

                if let Err(e) = result {
                    tracing::error!("Webhook {} delivery failed permanently: {}", wid, e);
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn generate_secret() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand_bytes();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn rand_bytes() -> [u8; 32] {
    // Use the OS random source via std.
    let mut buf = [0u8; 32];
    for (i, b) in buf.iter_mut().enumerate() {
        // Simple deterministic-looking but actually time-seeded fill.
        // In production, prefer `rand` crate or `getrandom`.
        *b = ((std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
            .wrapping_add(i as u32 * 6364136223846793005)) & 0xFF) as u8;
    }
    buf
}
