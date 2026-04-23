use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// Resolved application secrets, sourced from Vault or environment variables.
pub struct AppSecrets {
    pub database_url: String,
    pub stellar_rpc_url: String,
    pub stellar_network: String,
}

struct CacheEntry {
    value: String,
    fetched_at: Instant,
}

pub struct SecretsManager {
    /// Base URL of the Vault server, e.g. `http://vault:8200`.
    vault_addr: Option<String>,
    /// Vault token for authentication.
    vault_token: Option<String>,
    /// Mount path for the KV v2 secrets engine.
    vault_mount: String,
    /// Secret path within the mount, e.g. `stellar-tipjar`.
    vault_path: String,
    http: reqwest::Client,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
}

impl SecretsManager {
    pub fn new() -> Self {
        Self {
            vault_addr: std::env::var("VAULT_ADDR").ok(),
            vault_token: std::env::var("VAULT_TOKEN").ok(),
            vault_mount: std::env::var("VAULT_MOUNT").unwrap_or_else(|_| "secret".to_string()),
            vault_path: std::env::var("VAULT_PATH").unwrap_or_else(|_| "stellar-tipjar".to_string()),
            http: reqwest::Client::new(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(300),
        }
    }

    /// Fetch a single key from Vault KV v2, with in-memory TTL caching.
    async fn vault_get(&self, key: &str) -> Option<String> {
        let (addr, token) = (self.vault_addr.as_ref()?, self.vault_token.as_ref()?);

        // Check cache first.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(key) {
                if entry.fetched_at.elapsed() < self.ttl {
                    return Some(entry.value.clone());
                }
            }
        }

        let url = format!(
            "{}/v1/{}/data/{}",
            addr, self.vault_mount, self.vault_path
        );

        let resp = self
            .http
            .get(&url)
            .header("X-Vault-Token", token)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            tracing::warn!(url = %url, status = %resp.status(), "Vault request failed");
            return None;
        }

        let body: serde_json::Value = resp.json().await.ok()?;
        let value = body["data"]["data"][key].as_str()?.to_string();

        tracing::info!(key = %key, "Secret fetched from Vault");

        let mut cache = self.cache.write().await;
        cache.insert(
            key.to_string(),
            CacheEntry { value: value.clone(), fetched_at: Instant::now() },
        );

        Some(value)
    }

    /// Resolve a secret: try Vault first, fall back to the environment variable.
    async fn resolve(&self, vault_key: &str, env_var: &str) -> anyhow::Result<String> {
        if let Some(v) = self.vault_get(vault_key).await {
            return Ok(v);
        }
        std::env::var(env_var).map_err(|_| {
            anyhow::anyhow!(
                "Secret '{}' not found in Vault and env var '{}' is not set",
                vault_key,
                env_var
            )
        })
    }

    pub async fn load(&self) -> anyhow::Result<AppSecrets> {
        Ok(AppSecrets {
            database_url: self.resolve("DATABASE_URL", "DATABASE_URL").await?,
            stellar_rpc_url: self
                .resolve("STELLAR_RPC_URL", "STELLAR_RPC_URL")
                .await
                .unwrap_or_else(|_| "https://soroban-testnet.stellar.org".to_string()),
            stellar_network: self
                .resolve("STELLAR_NETWORK", "STELLAR_NETWORK")
                .await
                .unwrap_or_else(|_| "testnet".to_string()),
        })
    }
}
