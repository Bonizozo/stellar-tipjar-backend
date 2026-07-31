pub mod retry;
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
    /// Events to subscribe to, e.g. `["tip.created", "creator.updated"]`
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

/// A webhook signing secret — supports versioning for rotation.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct WebhookSecret {
    pub id: Uuid,
    pub webhook_id: Uuid,
    /// The raw secret value.
    pub secret: String,
    /// `true` for the primary (current) secret; `false` for a retiring secret
    /// kept alive during the rotation overlap window.
    pub is_primary: bool,
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
         SET url        = COALESCE($2, url),
             events     = COALESCE($3, events),
             enabled    = COALESCE($4, enabled),
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
// Secret rotation
// ---------------------------------------------------------------------------

/// Rotate the signing secret for a webhook.
///
/// The old secret is kept as a non-primary entry in `webhook_secrets` for the
/// duration of the overlap window so existing consumers continue to verify
/// deliveries while they update their configuration.
///
/// Any secrets older than `retain_count` (default 2) are pruned so the table
/// does not grow unbounded.
///
/// Returns `(new_primary_secret, retiring_secret_opt)`.
pub async fn rotate_secret(
    pool: &PgPool,
    webhook_id: Uuid,
    retain_count: i64,
) -> Result<(String, Option<String>), sqlx::Error> {
    let new_secret = generate_secret();

    // Fetch the current primary secret to make it the retiring one.
    let current: Option<(String,)> = sqlx::query_as("SELECT secret FROM webhooks WHERE id = $1")
        .bind(webhook_id)
        .fetch_optional(pool)
        .await?;

    let retiring = current.map(|(s,)| s);

    // Update the primary secret in the webhooks table.
    sqlx::query("UPDATE webhooks SET secret = $1, updated_at = NOW() WHERE id = $2")
        .bind(&new_secret)
        .bind(webhook_id)
        .execute(pool)
        .await?;

    // Persist the new primary in webhook_secrets.
    sqlx::query(
        "INSERT INTO webhook_secrets (webhook_id, secret, is_primary)
         VALUES ($1, $2, TRUE)",
    )
    .bind(webhook_id)
    .bind(&new_secret)
    .execute(pool)
    .await?;

    // Mark all other entries as non-primary.
    sqlx::query(
        "UPDATE webhook_secrets SET is_primary = FALSE
         WHERE webhook_id = $1 AND secret != $2",
    )
    .bind(webhook_id)
    .bind(&new_secret)
    .execute(pool)
    .await?;

    // Prune secrets older than retain_count.
    sqlx::query(
        "DELETE FROM webhook_secrets
         WHERE webhook_id = $1
           AND id NOT IN (
               SELECT id FROM webhook_secrets
               WHERE webhook_id = $1
               ORDER BY created_at DESC
               LIMIT $2
           )",
    )
    .bind(webhook_id)
    .bind(retain_count)
    .execute(pool)
    .await?;

    Ok((new_secret, retiring))
}

/// Retrieve all currently active secrets for a webhook (primary + retiring).
/// Used to build dual-secret `DeliveryContext` during the rotation window.
pub async fn active_secrets(pool: &PgPool, webhook_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT secret FROM webhook_secrets
         WHERE webhook_id = $1
         ORDER BY is_primary DESC, created_at DESC
         LIMIT 2",
    )
    .bind(webhook_id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        // Fallback: read from the webhooks table directly.
        let row: Option<(String,)> = sqlx::query_as("SELECT secret FROM webhooks WHERE id = $1")
            .bind(webhook_id)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|(s,)| vec![s]).unwrap_or_default())
    } else {
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }
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

        tracing::info!(
            event = %event_name,
            count = webhooks.len(),
            "Dispatching webhooks"
        );

        for webhook in webhooks {
            let pool2 = pool.clone();
            let event = WebhookEvent {
                id: Uuid::new_v4(),
                event_type: event_name.clone(),
                payload: payload.clone(),
                timestamp: Utc::now(),
            };
            let event_value = serde_json::to_value(&event).unwrap_or_default();
            let wid = webhook.id;

            tokio::spawn(async move {
                // Try to load all active secrets for dual-signing; fall back to
                // the single secret stored on the webhook row.
                let secrets = match active_secrets(&pool2, wid).await {
                    Ok(s) if !s.is_empty() => s,
                    _ => vec![webhook.secret.clone()],
                };

                let mut ctx =
                    sender::DeliveryContext::new(event.event_type.clone(), secrets[0].clone());
                if secrets.len() > 1 {
                    ctx.secrets = secrets;
                }

                use retry::{deliver_with_context, WebhookRetryConfig};
                let config = WebhookRetryConfig::default();
                let status =
                    deliver_with_context(&pool2, &webhook, ctx, event_value, &config).await;

                if !status.success {
                    tracing::error!(webhook_id = %wid, "Webhook permanently failed");
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a cryptographically suitable webhook secret using the OS entropy
/// source via `rand::thread_rng`.
pub fn generate_secret() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}
