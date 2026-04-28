//! Webhook retry mechanism with exponential backoff and dead letter queue.

use crate::metrics::collectors::{WEBHOOK_DELIVERIES_TOTAL, WEBHOOK_DLQ_TOTAL, WEBHOOK_RETRY_ATTEMPTS_TOTAL};
use crate::webhooks::{log_delivery, sender, Webhook};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// Retry configuration with exponential backoff
#[derive(Debug, Clone)]
pub struct WebhookRetryConfig {
    pub max_attempts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for WebhookRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay_ms: 1_000,
            max_delay_ms: 60_000,
            backoff_multiplier: 2.0,
        }
    }
}

impl WebhookRetryConfig {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let ms = (self.initial_delay_ms as f64
            * self.backoff_multiplier.powi(attempt as i32)) as u64;
        Duration::from_millis(ms.min(self.max_delay_ms))
    }
}

/// Dead letter queue entry for permanently failed webhooks
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeadLetterEntry {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub last_error: String,
    pub attempts: i32,
    pub created_at: DateTime<Utc>,
    pub failed_at: DateTime<Utc>,
}

/// Delivery status for tracking
#[derive(Debug, Serialize, Deserialize)]
pub struct DeliveryStatus {
    pub webhook_id: Uuid,
    pub event_type: String,
    pub success: bool,
    pub attempts: u32,
    pub last_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Deliver a webhook with exponential backoff retry.
/// On permanent failure, moves to dead letter queue.
pub async fn deliver_with_retry(
    pool: &PgPool,
    webhook: &Webhook,
    event_type: &str,
    payload: Value,
    config: &WebhookRetryConfig,
) -> DeliveryStatus {
    let mut last_error = String::new();
    let mut last_status_code: Option<i32> = None;

    for attempt in 0..config.max_attempts {
        if attempt > 0 {
            let delay = config.delay_for_attempt(attempt - 1);
            tracing::info!(
                webhook_id = %webhook.id,
                attempt,
                delay_ms = delay.as_millis(),
                "Retrying webhook delivery"
            );
            WEBHOOK_RETRY_ATTEMPTS_TOTAL
                .with_label_values(&[&attempt.to_string()])
                .inc();
            tokio::time::sleep(delay).await;
        }

        match sender::send_webhook(&webhook.url, &webhook.secret, payload.clone()).await {
            Ok(_) => {
                WEBHOOK_DELIVERIES_TOTAL.with_label_values(&["success"]).inc();
                let _ = log_delivery(
                    pool,
                    webhook.id,
                    event_type,
                    &payload,
                    Some(200),
                    None,
                    true,
                    (attempt + 1) as i32,
                )
                .await;

                tracing::info!(
                    webhook_id = %webhook.id,
                    attempt = attempt + 1,
                    "Webhook delivered successfully"
                );

                return DeliveryStatus {
                    webhook_id: webhook.id,
                    event_type: event_type.to_string(),
                    success: true,
                    attempts: attempt + 1,
                    last_status_code: Some(200),
                    last_error: None,
                    delivered_at: Some(Utc::now()),
                };
            }
            Err(e) => {
                last_error = e.to_string();
                WEBHOOK_DELIVERIES_TOTAL.with_label_values(&["failure"]).inc();
                tracing::warn!(
                    webhook_id = %webhook.id,
                    attempt = attempt + 1,
                    error = %e,
                    "Webhook delivery attempt failed"
                );
            }
        }
    }

    // All attempts exhausted — move to dead letter queue
    WEBHOOK_DLQ_TOTAL.inc();
    let _ = move_to_dlq(pool, webhook.id, event_type, &payload, &last_error, config.max_attempts as i32).await;
    let _ = log_delivery(
        pool,
        webhook.id,
        event_type,
        &payload,
        last_status_code,
        Some(&last_error),
        false,
        config.max_attempts as i32,
    )
    .await;

    tracing::error!(
        webhook_id = %webhook.id,
        attempts = config.max_attempts,
        error = last_error,
        "Webhook permanently failed, moved to DLQ"
    );

    DeliveryStatus {
        webhook_id: webhook.id,
        event_type: event_type.to_string(),
        success: false,
        attempts: config.max_attempts,
        last_status_code,
        last_error: Some(last_error),
        delivered_at: None,
    }
}

async fn move_to_dlq(
    pool: &PgPool,
    webhook_id: Uuid,
    event_type: &str,
    payload: &Value,
    last_error: &str,
    attempts: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO webhook_dead_letter_queue
            (id, webhook_id, event_type, payload, last_error, attempts, failed_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        "#,
        Uuid::new_v4(),
        webhook_id,
        event_type,
        payload,
        last_error,
        attempts,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// List dead letter queue entries
pub async fn list_dlq(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<DeadLetterEntry>, sqlx::Error> {
    sqlx::query_as::<_, DeadLetterEntry>(
        "SELECT * FROM webhook_dead_letter_queue ORDER BY failed_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Replay a dead letter queue entry (re-attempt delivery)
pub async fn replay_dlq_entry(
    pool: &PgPool,
    dlq_id: Uuid,
    config: &WebhookRetryConfig,
) -> Result<DeliveryStatus, sqlx::Error> {
    let entry = sqlx::query_as::<_, DeadLetterEntry>(
        "SELECT * FROM webhook_dead_letter_queue WHERE id = $1",
    )
    .bind(dlq_id)
    .fetch_one(pool)
    .await?;

    let webhook = crate::webhooks::get_webhook(pool, entry.webhook_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let status = deliver_with_retry(pool, &webhook, &entry.event_type, entry.payload, config).await;

    if status.success {
        sqlx::query!("DELETE FROM webhook_dead_letter_queue WHERE id = $1", dlq_id)
            .execute(pool)
            .await?;
    }

    Ok(status)
}
