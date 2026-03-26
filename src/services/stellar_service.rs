use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::errors::{AppError, AppResult, StellarError};
use super::circuit_breaker::CircuitBreaker;
use super::retry::{with_retry, RetryConfig};

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct HorizonTransactionResponse {
    pub id: String,
    pub hash: String,
    pub successful: bool,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct SorobanRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Clone)]
pub struct StellarService {
    client: Client,
    #[allow(dead_code)]
    pub rpc_url: String,
    pub network: String,
    retry_config: RetryConfig,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl StellarService {
    pub fn new(rpc_url: String, network: String) -> Self {
        Self {
            client: Client::new(),
            rpc_url,
            network,
            retry_config: RetryConfig::default(),
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(60))),
        }
    }

    /// Verify that a transaction exists and was successful on the Stellar network.
    ///
    /// Uses retry with exponential backoff for transient network errors and a
    /// circuit breaker to avoid hammering a failing Horizon server.
    pub async fn verify_transaction(
        &self,
        transaction_hash: &str,
    ) -> AppResult<bool> {
        if !self.circuit_breaker.allow_request() {
            tracing::warn!(
                "Circuit breaker open; skipping verification for {}",
                transaction_hash
            );
            return Err(AppError::Stellar(StellarError::CircuitBreakerOpen));
        }

        let horizon_base = if self.network == "mainnet" {
            "https://horizon.stellar.org"
        } else {
            "https://horizon-testnet.stellar.org"
        };

        let url = format!("{}/transactions/{}", horizon_base, transaction_hash);
        let client = self.client.clone();
        let cb = self.circuit_breaker.clone();

        let result = with_retry(&self.retry_config, || {
            let client = client.clone();
            let url = url.clone();
            async move {
                let resp = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

                if resp.status().is_success() {
                    let tx: HorizonTransactionResponse = resp
                        .json()
                        .await
                        .map_err(|_| {
                            AppError::Stellar(StellarError::InvalidTransaction {
                                reason: "Malformed Horizon response".to_string(),
                            })
                        })?;
                    Ok(tx.successful)
                } else if resp.status().as_u16() == 404 {
                    Ok(false)
                } else {
                    Err(AppError::Stellar(StellarError::NetworkUnavailable))
                }
            }
        })
        .await;

        match &result {
            Ok(_) => cb.record_success(),
            Err(_) => cb.record_failure(),
        }

        result
    }

    /// Get the current health of the Stellar network connection.
    #[allow(dead_code)]
    pub async fn get_network_health(&self) -> AppResult<serde_json::Value> {
        let req = SorobanRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "getHealth".to_string(),
            params: serde_json::Value::Null,
        };

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&req)
            .send()
            .await
            .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?
            .json::<serde_json::Value>()
            .await
            .map_err(|_| AppError::Stellar(StellarError::NetworkUnavailable))?;

        Ok(response)
    }
}
