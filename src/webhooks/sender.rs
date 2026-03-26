use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use crate::services::retry::{with_retry, RetryConfig};
use super::signature;

/// Configuration for webhook delivery.
const WEBHOOK_TIMEOUT_SECS: u64 = 5;

/// Sends a webhook notification once.
pub async fn send_webhook(url: &str, secret: &str, payload: Value) -> anyhow::Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_SECS))
        .build()?;
    
    let payload_str = serde_json::to_string(&payload)?;
    let signature = signature::generate_signature(secret, &payload_str);
    
    let response = client.post(url)
        .header("X-Webhook-Signature", signature)
        .header("Content-Type", "application/json")
        .body(payload_str)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Webhook delivery failed with status: {}", response.status()));
    }

    Ok(())
}

/// Sends a webhook notification with exponential backoff retry.
pub async fn send_webhook_with_retry(
    url: String, 
    secret: String, 
    payload: Value
) -> anyhow::Result<()> {
    let config = RetryConfig::default();
    
    with_retry(&config, || {
        let u = url.clone();
        let s = secret.clone();
        let p = payload.clone();
        async move { send_webhook(&u, &s, p).await }
    }).await.map_err(|e| anyhow::anyhow!("Webhook retry exhausted: {}", e))
}
