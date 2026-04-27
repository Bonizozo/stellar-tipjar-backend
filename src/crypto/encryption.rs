use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::{Aead, OsRng, rand_core::RngCore, Payload};
use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{Decode, Encode, Type, Postgres};
use sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef};
use sqlx::error::BoxDynError;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::metrics::collectors::{ENCRYPTION_OPERATIONS_TOTAL, ENCRYPTION_FAILURES_TOTAL, ENCRYPTION_KEY_ROTATIONS_TOTAL};

const FORMAT_PREFIX: &str = "enc:v1";
const NONCE_LEN: usize = 12;
const DEFAULT_KEY_TTL_SECS: u64 = 300;

static GLOBAL_ENCRYPTION_MANAGER: OnceLock<Arc<EncryptionKeyManager>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct EncryptionKeyManager {
    current_key_id: String,
    keys: HashMap<String, [u8; 32]>,
    vault_addr: Option<String>,
    vault_token: Option<String>,
    vault_mount: String,
    vault_path: String,
    http: Client,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
}

#[derive(Debug)]
struct CacheEntry {
    value: String,
    fetched_at: Instant,
}

/// A string that is encrypted before it is persisted and decrypted when loaded.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct EncryptedString(String);

impl EncryptedString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for EncryptedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<EncryptedString> for String {
    fn from(value: EncryptedString) -> Self {
        value.0
    }
}

impl AsRef<str> for EncryptedString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for EncryptedString {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Type<Postgres> for EncryptedString {
    fn type_info() -> PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }
}

impl PgHasArrayType for EncryptedString {
    fn array_type_info() -> PgTypeInfo {
        <String as PgHasArrayType>::array_type_info()
    }
}

impl<'r> Decode<'r, Postgres> for EncryptedString {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let raw = <String as Decode<Postgres>>::decode(value)?;
        let manager = global_encryption_manager().map_err(|e| e.into())?;
        let decrypted = manager.decrypt_to_string(&raw).map_err(|e| e.into())?;
        Ok(EncryptedString::new(decrypted))
    }
}

impl<'q> Encode<'q, Postgres> for EncryptedString {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> sqlx::encode::IsNull {
        let manager = global_encryption_manager().expect("encryption manager must be initialized before DB operations");
        let encrypted = manager
            .encrypt_to_string(self.as_str())
            .expect("failed to encrypt field before persistence");
        <String as Encode<Postgres>>::encode_by_ref(&encrypted, buf)
    }
}

impl EncryptionKeyManager {
    pub fn new() -> Self {
        Self {
            current_key_id: String::new(),
            keys: HashMap::new(),
            vault_addr: std::env::var("VAULT_ADDR").ok(),
            vault_token: std::env::var("VAULT_TOKEN").ok(),
            vault_mount: std::env::var("VAULT_MOUNT").unwrap_or_else(|_| "secret".to_string()),
            vault_path: std::env::var("VAULT_PATH").unwrap_or_else(|_| "stellar-tipjar".to_string()),
            http: Client::new(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(DEFAULT_KEY_TTL_SECS),
        }
    }

    async fn vault_get(&self, key: &str) -> Option<String> {
        let (addr, token) = (self.vault_addr.as_ref()?, self.vault_token.as_ref()?);

        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(key) {
                if entry.fetched_at.elapsed() < self.ttl {
                    return Some(entry.value.clone());
                }
            }
        }

        let url = format!("{}/v1/{}/data/{}", addr, self.vault_mount, self.vault_path);
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
        let secret = body["data"]["data"][key].as_str()?.to_string();

        let mut cache = self.cache.write().await;
        cache.insert(
            key.to_string(),
            CacheEntry { value: secret.clone(), fetched_at: Instant::now() },
        );

        Some(secret)
    }

    async fn resolve_key(&self, key_name: &str) -> anyhow::Result<String> {
        if let Some(secret) = self.vault_get(key_name).await {
            return Ok(secret);
        }

        std::env::var(key_name).map_err(|_| {
            anyhow::anyhow!(
                "Encryption key '{}' not found in Vault and env var '{}' is not set",
                key_name,
                key_name
            )
        })
    }

    pub async fn load(mut self) -> anyhow::Result<Self> {
        let key_ids = if let Ok(ids) = std::env::var("ENCRYPTION_KEY_IDS") {
            ids.split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect()
        } else {
            vec!["current".to_string(), "old_1".to_string(), "old_2".to_string()]
        };

        for key_id in key_ids.iter() {
            let key_name = format!("ENCRYPTION_KEY_{}", key_id.to_uppercase());
            if let Ok(raw_key) = self.resolve_key(&key_name).await {
                let key = parse_key(&raw_key)?;
                self.keys.insert(key_id.clone(), key);
            }
        }

        if self.keys.is_empty() {
            return Err(anyhow::anyhow!(
                "No encryption keys found. Set ENCRYPTION_KEY_CURRENT or ENCRYPTION_KEY_IDS with env or Vault secrets."
            ));
        }

        self.current_key_id = if self.keys.contains_key("current") {
            "current".to_string()
        } else {
            self.keys.keys().next().cloned().unwrap()
        };

        Ok(self)
    }

    pub fn active_key_id(&self) -> &str {
        &self.current_key_id
    }

    pub fn encrypt_to_string(&self, plaintext: &str) -> anyhow::Result<String> {
        let key = self
            .keys
            .get(&self.current_key_id)
            .ok_or_else(|| anyhow::anyhow!("Encryption active key is unavailable"))?;

        let cipher = Aes256Gcm::new_from_slice(key).expect("valid AES-256-GCM key");
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| {
                ENCRYPTION_FAILURES_TOTAL.with_label_values(&["encrypt"]).inc();
                anyhow::anyhow!("encryption failed: {}", e)
            })?;

        let payload = format!(
            "{}:{}:{}:{}",
            FORMAT_PREFIX,
            self.current_key_id,
            general_purpose::URL_SAFE_NO_PAD.encode(nonce_bytes),
            general_purpose::URL_SAFE_NO_PAD.encode(ciphertext)
        );

        ENCRYPTION_OPERATIONS_TOTAL.with_label_values(&["encrypt"]).inc();
        Ok(payload)
    }

    pub fn decrypt_to_string(&self, encrypted_value: &str) -> anyhow::Result<String> {
        let parts: Vec<&str> = encrypted_value.splitn(4, ':').collect();
        if parts.len() != 4 || parts[0] != FORMAT_PREFIX {
            ENCRYPTION_FAILURES_TOTAL.with_label_values(&["invalid_format"]).inc();
            return Err(anyhow::anyhow!("Invalid encrypted payload format"));
        }

        let key_id = parts[1];
        let nonce = general_purpose::URL_SAFE_NO_PAD.decode(parts[2])
            .map_err(|e| {
                ENCRYPTION_FAILURES_TOTAL.with_label_values(&["decode_nonce"]).inc();
                e
            })?;
        let ciphertext = general_purpose::URL_SAFE_NO_PAD.decode(parts[3])
            .map_err(|e| {
                ENCRYPTION_FAILURES_TOTAL.with_label_values(&["decode_ciphertext"]).inc();
                e
            })?;
        let key = self
            .keys
            .get(key_id)
            .ok_or_else(|| {
                ENCRYPTION_FAILURES_TOTAL.with_label_values(&["key_not_found"]).inc();
                anyhow::anyhow!("Encryption key '{}' is not available", key_id)
            })?;

        let cipher = Aes256Gcm::new_from_slice(key).expect("valid AES-256-GCM key");
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|e| {
                ENCRYPTION_FAILURES_TOTAL.with_label_values(&["decrypt"]).inc();
                anyhow::anyhow!("decryption failed: {}", e)
            })?;

        ENCRYPTION_OPERATIONS_TOTAL.with_label_values(&["decrypt"]).inc();
        Ok(String::from_utf8(plaintext)?)
    }

    /// Rotate encryption keys by generating a new current key and archiving the old one.
    /// This method updates the key manager in place and should be called periodically.
    pub async fn rotate_keys(&mut self) -> anyhow::Result<()> {
        // Generate a new key for "current"
        let mut new_key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut new_key_bytes);

        // Archive the current key as "old_1", shift existing old keys
        let mut new_keys = HashMap::new();
        new_keys.insert("current".to_string(), new_key_bytes);

        // Move existing keys to old positions
        if let Some(current_key) = self.keys.get("current") {
            new_keys.insert("old_1".to_string(), *current_key);
        }
        if let Some(old_1_key) = self.keys.get("old_1") {
            new_keys.insert("old_2".to_string(), *old_1_key);
        }
        // Keep old_2 if it exists, or drop it

        self.keys = new_keys;
        self.current_key_id = "current".to_string();

        // Update environment variables or Vault with new keys
        // Note: In a real implementation, you'd want to persist these securely
        // For now, we'll assume they're set via environment or Vault

        ENCRYPTION_KEY_ROTATIONS_TOTAL.inc();
        tracing::info!("Encryption keys rotated successfully");
        Ok(())
    }
}

pub fn set_global_encryption_manager(manager: Arc<EncryptionKeyManager>) -> anyhow::Result<()> {
    GLOBAL_ENCRYPTION_MANAGER
        .set(manager)
        .map_err(|_| anyhow::anyhow!("Global encryption manager is already initialized"))
}

pub fn global_encryption_manager() -> anyhow::Result<&'static Arc<EncryptionKeyManager>> {
    GLOBAL_ENCRYPTION_MANAGER
        .get()
        .ok_or_else(|| anyhow::anyhow!("Global encryption manager is not initialized"))
}

fn parse_key(raw: &str) -> anyhow::Result<[u8; 32]> {
    let decoded = if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        hex::decode(raw)?
    } else {
        general_purpose::STANDARD.decode(raw)?
    };

    if decoded.len() != 32 {
        return Err(anyhow::anyhow!(
            "Encryption key must be 32 bytes (256 bits); got {} bytes",
            decoded.len()
        ));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn encryption_round_trip() {
        std::env::remove_var("ENCRYPTION_KEY_CURRENT");
        std::env::set_var("ENCRYPTION_KEY_CURRENT", "0000000000000000000000000000000000000000000000000000000000000000");

        let manager = EncryptionKeyManager::new().load().await.expect("load key manager");
        set_global_encryption_manager(Arc::new(manager.clone())).unwrap();

        let plaintext = "test-secret";
        let encrypted = manager.encrypt_to_string(plaintext).unwrap();
        assert!(encrypted.starts_with(FORMAT_PREFIX));
        let decrypted = manager.decrypt_to_string(&encrypted).unwrap();
        assert_eq!(plaintext, decrypted);
    }
}
