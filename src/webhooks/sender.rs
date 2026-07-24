//! Outbound webhook delivery with versioned signatures and stable delivery IDs.

use crate::telemetry::http_client::inject_trace_headers;
use crate::webhooks::signature;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tracing::Instrument;
use uuid::Uuid;

/// Per-request delivery context — persisted so the same `delivery_id` is
/// reused on every retry, enabling receiver-side deduplication.
#[derive(Debug, Clone)]
pub struct DeliveryContext {
    /// Stable UUID for this logical delivery attempt (same across retries).
    pub delivery_id: Uuid,
    /// The event type being delivered (e.g. `"tip.created"`).
    pub event_type: String,
    /// Active signing secrets — two during rotation, one otherwise.
    pub secrets: Vec<String>,
}

impl DeliveryContext {
    /// Create a new context with a freshly-generated delivery ID.
    pub fn new(event_type: impl Into<String>, secret: String) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            event_type: event_type.into(),
            secrets: vec![secret],
        }
    }

    /// Create with two secrets for the rotation overlap period.
    pub fn with_rotation(
        event_type: impl Into<String>,
        primary_secret: String,
        retiring_secret: String,
    ) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            event_type: event_type.into(),
            secrets: vec![primary_secret, retiring_secret],
        }
    }
}

/// Timeout applied to each outbound webhook POST.
const WEBHOOK_TIMEOUT_SECS: u64 = 10;

/// Send one webhook delivery attempt.
///
/// Sets the following headers on every request:
/// - `X-TipJar-Signature`   — `t=<ts>,v1=<hmac>[,v1=<hmac2>]`
/// - `X-TipJar-Delivery-Id` — stable UUID (same across retries)
/// - `X-TipJar-Event-Type`  — e.g. `"tip.created"`
/// - `Content-Type`         — `application/json`
/// - `traceparent`          — W3C distributed trace context
///
/// Returns the HTTP status code on success so callers can record it.
pub async fn send_webhook(
    url: &str,
    ctx: &DeliveryContext,
    payload: Value,
) -> anyhow::Result<u16> {
    let client = Client::builder()
        .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_SECS))
        .build()?;

    let payload_str = serde_json::to_string(&payload)?;
    let secret_refs: Vec<&str> = ctx.secrets.iter().map(|s| s.as_str()).collect();
    let sig_header = signature::build_signature_header(&secret_refs, &payload_str);

    let span = tracing::info_span!(
        "webhook.deliver",
        "http.url"          = %url,
        "http.method"       = "POST",
        "peer.service"      = "webhook_target",
        "webhook.delivery"  = %ctx.delivery_id,
        "webhook.event"     = %ctx.event_type,
    );

    async move {
        let mut headers = reqwest::header::HeaderMap::new();
        inject_trace_headers(&mut headers);

        headers.insert(
            "X-TipJar-Signature",
            reqwest::header::HeaderValue::from_str(&sig_header)
                .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
        );
        headers.insert(
            "X-TipJar-Delivery-Id",
            reqwest::header::HeaderValue::from_str(&ctx.delivery_id.to_string())
                .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
        );
        headers.insert(
            "X-TipJar-Event-Type",
            reqwest::header::HeaderValue::from_str(&ctx.event_type)
                .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let response = client
            .post(url)
            .headers(headers)
            .body(payload_str)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Webhook delivery failed with status: {}",
                status
            ));
        }

        Ok(status.as_u16())
    }
    .instrument(span)
    .await
}

/// Compatibility shim used by `trigger_webhooks` in `mod.rs`.
///
/// Creates a single-secret `DeliveryContext` and delegates to `send_webhook`.
pub async fn send_webhook_with_retry(
    url: String,
    secret: String,
    payload: Value,
) -> anyhow::Result<()> {
    use crate::services::retry::{with_retry, RetryConfig};

    let ctx = DeliveryContext::new("webhook.event", secret);
    let config = RetryConfig::default();

    with_retry(&config, || {
        let u = url.clone();
        let c = ctx.clone();
        let p = payload.clone();
        async move { send_webhook(&u, &c, p).await.map(|_| ()) }
    })
    .await
    .map_err(|e| anyhow::anyhow!("Webhook retry exhausted: {}", e))
}
