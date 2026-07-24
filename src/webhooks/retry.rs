//! Webhook retry engine with exponential backoff + jitter, per-endpoint
//! circuit breaking, stable delivery IDs, and dead-letter re-drive.

use crate::metrics::collectors::{WEBHOOK_DELIVERIES_TOTAL, WEBHOOK_DLQ_TOTAL, WEBHOOK_RETRY_ATTEMPTS_TOTAL};
use crate::services::circuit_breaker::{CircuitBreaker, CircuitState};
use crate::webhooks::{log_delivery, sender::{DeliveryContext, send_webhook}, Webhook};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

// ── Retry configuration ────────────────────────────────────────────────────────

/// Exponential backoff with full jitter.
///
/// Delay for attempt `n`:
/// ```text
/// capped = min(initial_delay_ms * backoff_multiplier^n, max_delay_ms)
/// actual = random_in(0, capped)    ← full jitter
/// ```
#[derive(Debug, Clone)]
pub struct WebhookRetryConfig {
    pub max_attempts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
    /// Circuit breaker: number of consecutive failures before opening.
    pub circuit_failure_threshold: u32,
    /// How long the circuit stays open before allowing a probe.
    pub circuit_recovery_secs: u64,
}

impl Default for WebhookRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay_ms: 1_000,
            max_delay_ms: 60_000,
            backoff_multiplier: 2.0,
            circuit_failure_threshold: 5,
            circuit_recovery_secs: 300, // 5 minutes
        }
    }
}

impl WebhookRetryConfig {
    /// Compute the capped deterministic delay for attempt `n` (pre-jitter).
    pub fn base_delay_ms(&self, attempt: u32) -> u64 {
        let ms = (self.initial_delay_ms as f64
            * self.backoff_multiplier.powi(attempt as i32)) as u64;
        ms.min(self.max_delay_ms)
    }

    /// Compute the actual delay with full jitter: `random_in(0, base)`.
    pub fn jittered_delay(&self, attempt: u32) -> Duration {
        let base = self.base_delay_ms(attempt);
        // Cheap pseudo-random using the nanosecond clock.
        let jitter = if base > 0 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64;
            nanos % (base + 1)
        } else {
            0
        };
        Duration::from_millis(jitter)
    }
}

// ── Per-endpoint circuit breaker registry ────────────────────────────────────

/// Global registry of circuit breakers keyed by webhook URL.
///
/// Shared across all delivery tasks in the process so a consistently
/// failing endpoint is quarantined regardless of which task drives it.
static ENDPOINT_CIRCUITS: Mutex<Option<HashMap<String, Arc<CircuitBreaker>>>> =
    Mutex::new(None);

fn get_or_create_circuit(url: &str, config: &WebhookRetryConfig) -> Arc<CircuitBreaker> {
    let mut guard = ENDPOINT_CIRCUITS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.entry(url.to_string())
        .or_insert_with(|| {
            Arc::new(CircuitBreaker::new(
                config.circuit_failure_threshold,
                Duration::from_secs(config.circuit_recovery_secs),
            ))
        })
        .clone()
}

// ── Delivery types ─────────────────────────────────────────────────────────────

/// Dead-letter queue entry for permanently failed webhooks.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeadLetterEntry {
    pub id: Uuid,
    pub webhook_id: Uuid,
    /// Stable delivery ID — same UUID that was used during the original
    /// attempts so the receiver can correlate with its own logs.
    pub delivery_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub last_error: String,
    pub attempts: i32,
    pub created_at: DateTime<Utc>,
    pub failed_at: DateTime<Utc>,
}

/// Summary returned after a complete delivery attempt sequence.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeliveryStatus {
    pub webhook_id: Uuid,
    /// Stable delivery ID — unchanged across every retry.
    pub delivery_id: Uuid,
    pub event_type: String,
    pub success: bool,
    pub attempts: u32,
    pub last_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
}

// ── Core delivery function ─────────────────────────────────────────────────────

/// Deliver a webhook with jittered exponential backoff and per-endpoint
/// circuit breaking.
///
/// The `delivery_id` in `ctx` is **stable** — it is reused on every retry so
/// the receiver can deduplicate redelivered events.
///
/// On permanent failure (all attempts exhausted or circuit open) the delivery
/// is moved to the dead-letter queue.
pub async fn deliver_with_retry(
    pool: &PgPool,
    webhook: &Webhook,
    event_type: &str,
    payload: Value,
    config: &WebhookRetryConfig,
) -> DeliveryStatus {
    let ctx = DeliveryContext::new(event_type, webhook.secret.clone());
    deliver_with_context(pool, webhook, ctx, payload, config).await
}

/// Same as `deliver_with_retry` but accepts a pre-built `DeliveryContext`,
/// allowing the caller to supply dual secrets during rotation or a
/// pre-existing `delivery_id` when re-driving a DLQ entry.
pub async fn deliver_with_context(
    pool: &PgPool,
    webhook: &Webhook,
    ctx: DeliveryContext,
    payload: Value,
    config: &WebhookRetryConfig,
) -> DeliveryStatus {
    let circuit = get_or_create_circuit(&webhook.url, config);
    let mut last_error = String::new();
    let mut last_status_code: Option<i32> = None;

    // Fail-fast when the circuit is fully open (not half-open).
    if circuit.state() == CircuitState::Open {
        let msg = format!(
            "circuit breaker OPEN for endpoint {}; skipping delivery",
            &webhook.url
        );
        tracing::warn!(webhook_id = %webhook.id, url = %webhook.url, "{msg}");
        let _ = move_to_dlq(
            pool,
            webhook.id,
            ctx.delivery_id,
            event_type,
            &payload,
            &msg,
            0,
        )
        .await;
        WEBHOOK_DLQ_TOTAL.inc();
        return DeliveryStatus {
            webhook_id: webhook.id,
            delivery_id: ctx.delivery_id,
            event_type: event_type.to_string(),
            success: false,
            attempts: 0,
            last_status_code: None,
            last_error: Some(msg),
            delivered_at: None,
        };
    }

    let event_type = ctx.event_type.clone();

    for attempt in 0..config.max_attempts {
        if attempt > 0 {
            let delay = config.jittered_delay(attempt - 1);
            tracing::info!(
                webhook_id   = %webhook.id,
                delivery_id  = %ctx.delivery_id,
                attempt,
                delay_ms     = delay.as_millis(),
                "Retrying webhook delivery"
            );
            WEBHOOK_RETRY_ATTEMPTS_TOTAL
                .with_label_values(&[&attempt.to_string()])
                .inc();
            tokio::time::sleep(delay).await;

            // Re-check circuit after sleep — it may have opened during the wait.
            if circuit.state() == CircuitState::Open {
                last_error = format!(
                    "circuit breaker OPEN after attempt {}; aborting retries",
                    attempt
                );
                break;
            }
        }

        match send_webhook(&webhook.url, &ctx, payload.clone()).await {
            Ok(status) => {
                circuit.record_success();
                WEBHOOK_DELIVERIES_TOTAL.with_label_values(&["success"]).inc();
                last_status_code = Some(status as i32);

                let _ = log_delivery(
                    pool,
                    webhook.id,
                    &event_type,
                    &payload,
                    Some(status as i32),
                    None,
                    true,
                    (attempt + 1) as i32,
                )
                .await;

                tracing::info!(
                    webhook_id  = %webhook.id,
                    delivery_id = %ctx.delivery_id,
                    attempt     = attempt + 1,
                    status,
                    "Webhook delivered successfully"
                );

                return DeliveryStatus {
                    webhook_id: webhook.id,
                    delivery_id: ctx.delivery_id,
                    event_type: event_type.clone(),
                    success: true,
                    attempts: attempt + 1,
                    last_status_code: Some(status as i32),
                    last_error: None,
                    delivered_at: Some(Utc::now()),
                };
            }
            Err(e) => {
                circuit.record_failure();
                last_error = e.to_string();
                WEBHOOK_DELIVERIES_TOTAL.with_label_values(&["failure"]).inc();
                tracing::warn!(
                    webhook_id  = %webhook.id,
                    delivery_id = %ctx.delivery_id,
                    attempt     = attempt + 1,
                    error       = %e,
                    "Webhook delivery attempt failed"
                );
            }
        }
    }

    // All attempts exhausted — persist to DLQ.
    WEBHOOK_DLQ_TOTAL.inc();
    let _ = move_to_dlq(
        pool,
        webhook.id,
        ctx.delivery_id,
        &event_type,
        &payload,
        &last_error,
        config.max_attempts as i32,
    )
    .await;
    let _ = log_delivery(
        pool,
        webhook.id,
        &event_type,
        &payload,
        last_status_code,
        Some(&last_error),
        false,
        config.max_attempts as i32,
    )
    .await;

    tracing::error!(
        webhook_id  = %webhook.id,
        delivery_id = %ctx.delivery_id,
        attempts    = config.max_attempts,
        error       = %last_error,
        "Webhook permanently failed, moved to DLQ"
    );

    DeliveryStatus {
        webhook_id: webhook.id,
        delivery_id: ctx.delivery_id,
        event_type: event_type.clone(),
        success: false,
        attempts: config.max_attempts,
        last_status_code,
        last_error: Some(last_error),
        delivered_at: None,
    }
}

// ── DLQ operations ─────────────────────────────────────────────────────────────

async fn move_to_dlq(
    pool: &PgPool,
    webhook_id: Uuid,
    delivery_id: Uuid,
    event_type: &str,
    payload: &Value,
    last_error: &str,
    attempts: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO webhook_dead_letter_queue
            (id, webhook_id, delivery_id, event_type, payload, last_error, attempts, failed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        ON CONFLICT (delivery_id) DO UPDATE
            SET last_error = EXCLUDED.last_error,
                attempts   = EXCLUDED.attempts,
                failed_at  = NOW()
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(webhook_id)
    .bind(delivery_id)
    .bind(event_type)
    .bind(payload)
    .bind(last_error)
    .bind(attempts)
    .execute(pool)
    .await?;
    Ok(())
}

/// List dead-letter queue entries.
pub async fn list_dlq(pool: &PgPool, limit: i64) -> Result<Vec<DeadLetterEntry>, sqlx::Error> {
    sqlx::query_as::<_, DeadLetterEntry>(
        "SELECT * FROM webhook_dead_letter_queue ORDER BY failed_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Re-drive a DLQ entry — reuses the original `delivery_id` so receivers
/// can recognise the event as a re-delivery and deduplicate if desired.
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

    // Reuse the original delivery_id so the receiver can deduplicate.
    let mut ctx = DeliveryContext::new(&entry.event_type, webhook.secret.clone());
    ctx.delivery_id = entry.delivery_id;

    let status =
        deliver_with_context(pool, &webhook, ctx, entry.payload, config).await;

    if status.success {
        sqlx::query("DELETE FROM webhook_dead_letter_queue WHERE id = $1")
            .bind(dlq_id)
            .execute(pool)
            .await?;
    }

    Ok(status)
}

// ── Circuit breaker access (for tests) ────────────────────────────────────────

/// Reset the in-process circuit-breaker state for a URL.  Tests use this to
/// isolate circuit state between test cases.
#[cfg(test)]
pub fn reset_circuit_for_url(url: &str) {
    let mut guard = ENDPOINT_CIRCUITS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(map) = guard.as_mut() {
        map.remove(url);
    }
}
