use reqwest::Client;
use serde::{Deserialize, Serialize};

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

pub struct StellarService {
    client: Client,
    #[allow(dead_code)]
    pub rpc_url: String,
    pub network: String,
}

impl StellarService {
    pub fn new(rpc_url: String, network: String) -> Self {
        Self {
            client: Client::new(),
            rpc_url,
            network,
        }
    }

    /// Verify that a transaction exists and was successful on the Stellar network.
    #[tracing::instrument(skip(self), fields(network = %self.network))]
    pub async fn verify_transaction(
        &self,
        transaction_hash: &str,
    ) -> anyhow::Result<bool> {
        let horizon_base = if self.network == "mainnet" {
            "https://horizon.stellar.org"
        } else {
            "https://horizon-testnet.stellar.org"
        };

        let url = format!("{}/transactions/{}", horizon_base, transaction_hash);
        let response = self.client.get(&url).send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let tx: HorizonTransactionResponse = resp.json().await?;
                tracing::info!(successful = tx.successful, "Transaction verified");
                Ok(tx.successful)
            }
            Ok(resp) => {
                tracing::warn!(status = %resp.status(), "Transaction not found on Stellar");
                Ok(false)
            }
            Err(e) => {
                tracing::warn!("Failed to verify transaction {}: {}", transaction_hash, e);
                Ok(false)
            }
        }
    }

    /// Get the current health of the Stellar network connection.
    #[allow(dead_code)]
    pub async fn get_network_health(&self) -> anyhow::Result<serde_json::Value> {
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
            .await?
            .json::<serde_json::Value>()
            .await?;

        Ok(response)
    }
}
