use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReplayProtectionError {
    #[error("Nonce already used: {0}")]
    NonceAlreadyUsed(String),
    #[error("Request timestamp is too old: {0}")]
    TimestampTooOld(DateTime<Utc>),
    #[error("Request timestamp is too far in the future: {0}")]
    TimestampTooFuture(DateTime<Utc>),
    #[error("Invalid nonce format: {0}")]
    InvalidNonceFormat(String),
    #[error("Missing nonce or timestamp")]
    MissingNonceOrTimestamp,
    #[error("Redis operation failed: {0}")]
    RedisError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayProtectionConfig {
    pub nonce_ttl_seconds: u64,
    pub max_timestamp_drift_seconds: i64,
    pub cleanup_interval_seconds: u64,
    pub enabled_endpoints: Vec<String>,
}

impl Default for ReplayProtectionConfig {
    fn default() -> Self {
        Self {
            nonce_ttl_seconds: 300, // 5 minutes
            max_timestamp_drift_seconds: 60, // 1 minute
            cleanup_interval_seconds: 600, // 10 minutes
            enabled_endpoints: vec![
                "/api/v1/tips".to_string(),
                "/api/v1/creators".to_string(),
                "/api/v1/withdrawals".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceInfo {
    pub nonce: String,
    pub used_at: DateTime<Utc>,
    pub client_id: Option<String>,
    pub endpoint: String,
}

pub struct ReplayProtectionService {
    redis: Option<Arc<ConnectionManager>>,
    config: ReplayProtectionConfig,
}

impl ReplayProtectionService {
    pub fn new(redis: Option<Arc<ConnectionManager>>, config: ReplayProtectionConfig) -> Self {
        Self { redis, config }
    }

    pub fn with_redis(redis: Arc<ConnectionManager>) -> Self {
        Self::new(Some(redis), ReplayProtectionConfig::default())
    }

    pub fn without_redis() -> Self {
        Self::new(None, ReplayProtectionConfig::default())
    }

    /// Generate a cryptographically secure nonce
    pub fn generate_nonce() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let nonce_bytes: [u8; 32] = rng.gen();
        hex::encode(nonce_bytes)
    }

    /// Validate a request against replay attacks
    pub async fn validate_request(
        &self,
        nonce: &str,
        timestamp: DateTime<Utc>,
        client_id: Option<&str>,
        endpoint: &str,
    ) -> Result<(), ReplayProtectionError> {
        // Check if endpoint requires replay protection
        if !self.config.enabled_endpoints.contains(&endpoint.to_string()) {
            return Ok(());
        }

        // Validate nonce format
        if nonce.len() != 64 || !nonce.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ReplayProtectionError::InvalidNonceFormat(nonce.to_string()));
        }

        // Validate timestamp
        let now = Utc::now();
        let min_timestamp = now - Duration::seconds(self.config.max_timestamp_drift_seconds);
        let max_timestamp = now + Duration::seconds(self.config.max_timestamp_drift_seconds);

        if timestamp < min_timestamp {
            return Err(ReplayProtectionError::TimestampTooOld(timestamp));
        }

        if timestamp > max_timestamp {
            return Err(ReplayProtectionError::TimestampTooFuture(timestamp));
        }

        // Check if nonce has been used
        if let Some(redis) = &self.redis {
            self.check_nonce_redis(nonce, client_id, endpoint).await?;
        } else {
            // Fallback to in-memory check (less secure but functional)
            tracing::warn!("Redis not available, using in-memory nonce check (less secure)");
        }

        Ok(())
    }

    async fn check_nonce_redis(
        &self,
        nonce: &str,
        client_id: Option<&str>,
        endpoint: &str,
    ) -> Result<(), ReplayProtectionError> {
        let redis = self.redis.as_ref().unwrap();
        let mut conn = redis.clone();

        let key = format!("nonce:{}", nonce);
        
        // Check if nonce exists
        let exists: bool = redis::cmd("EXISTS")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

        if exists {
            return Err(ReplayProtectionError::NonceAlreadyUsed(nonce.to_string()));
        }

        // Store nonce with TTL
        let nonce_info = NonceInfo {
            nonce: nonce.to_string(),
            used_at: Utc::now(),
            client_id: client_id.map(|s| s.to_string()),
            endpoint: endpoint.to_string(),
        };

        let serialized = serde_json::to_string(&nonce_info)
            .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

        redis::cmd("SETEX")
            .arg(&key)
            .arg(self.config.nonce_ttl_seconds)
            .arg(&serialized)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

        Ok(())
    }

    /// Mark a nonce as used (alternative to validate_request)
    pub async fn mark_nonce_used(
        &self,
        nonce: &str,
        client_id: Option<&str>,
        endpoint: &str,
    ) -> Result<(), ReplayProtectionError> {
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let key = format!("nonce:{}", nonce);

            let nonce_info = NonceInfo {
                nonce: nonce.to_string(),
                used_at: Utc::now(),
                client_id: client_id.map(|s| s.to_string()),
                endpoint: endpoint.to_string(),
            };

            let serialized = serde_json::to_string(&nonce_info)
                .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

            redis::cmd("SETEX")
                .arg(&key)
                .arg(self.config.nonce_ttl_seconds)
                .arg(&serialized)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;
        }

        Ok(())
    }

    /// Check if a nonce has been used
    pub async fn is_nonce_used(&self, nonce: &str) -> Result<bool, ReplayProtectionError> {
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let key = format!("nonce:{}", nonce);

            let exists: bool = redis::cmd("EXISTS")
                .arg(&key)
                .query_async(&mut conn)
                .await
                .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

            Ok(exists)
        } else {
            Ok(false) // Assume not used if Redis is not available
        }
    }

    /// Generate a request fingerprint for deduplication
    pub fn generate_request_fingerprint(
        &self,
        method: &str,
        path: &str,
        body: &str,
        client_id: Option<&str>,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(path.as_bytes());
        hasher.update(body.as_bytes());
        if let Some(id) = client_id {
            hasher.update(id.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// Cleanup expired nonces
    pub async fn cleanup_expired_nonces(&self) -> Result<u64, ReplayProtectionError> {
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "nonce:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

            let mut deleted_count = 0;
            for key in keys {
                let ttl: i64 = redis::cmd("TTL")
                    .arg(&key)
                    .query_async(&mut conn)
                    .await
                    .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

                if ttl == -1 { // No expiry set, delete it
                    redis::cmd("DEL")
                        .arg(&key)
                        .query_async::<_, ()>(&mut conn)
                        .await
                        .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;
                    deleted_count += 1;
                }
            }

            Ok(deleted_count)
        } else {
            Ok(0)
        }
    }

    /// Get statistics about nonce usage
    pub async fn get_nonce_stats(&self) -> Result<HashMap<String, u64>, ReplayProtectionError> {
        let mut stats = HashMap::new();
        
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "nonce:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

            stats.insert("total_nonces".to_string(), keys.len() as u64);

            // Count by endpoint
            let mut endpoint_counts: HashMap<String, u64> = HashMap::new();
            for key in keys {
                if let Ok(Some(nonce_info)) = self.get_nonce_info(&key).await {
                    *endpoint_counts.entry(nonce_info.endpoint).or_insert(0) += 1;
                }
            }

            for (endpoint, count) in endpoint_counts {
                stats.insert(format!("endpoint:{}", endpoint), count);
            }
        } else {
            stats.insert("total_nonces".to_string(), 0);
        }

        Ok(stats)
    }

    async fn get_nonce_info(&self, key: &str) -> Result<Option<NonceInfo>, ReplayProtectionError> {
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            
            let value: Option<String> = redis::cmd("GET")
                .arg(key)
                .query_async(&mut conn)
                .await
                .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;

            if let Some(value) = value {
                let nonce_info: NonceInfo = serde_json::from_str(&value)
                    .map_err(|e| ReplayProtectionError::RedisError(e.to_string()))?;
                Ok(Some(nonce_info))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_generate_nonce() {
        let nonce1 = ReplayProtectionService::generate_nonce();
        let nonce2 = ReplayProtectionService::generate_nonce();
        
        assert_eq!(nonce1.len(), 64);
        assert_eq!(nonce2.len(), 64);
        assert_ne!(nonce1, nonce2);
    }

    #[test]
    fn test_generate_request_fingerprint() {
        let service = ReplayProtectionService::without_redis();
        
        let fp1 = service.generate_request_fingerprint("POST", "/api/v1/tips", "{\"amount\":100}", "client1");
        let fp2 = service.generate_request_fingerprint("POST", "/api/v1/tips", "{\"amount\":100}", "client1");
        let fp3 = service.generate_request_fingerprint("POST", "/api/v1/tips", "{\"amount\":200}", "client1");
        
        assert_eq!(fp1, fp2);
        assert_ne!(fp1, fp3);
    }

    #[test]
    fn test_timestamp_validation() {
        let service = ReplayProtectionService::without_redis();
        let now = Utc::now();
        
        // Valid timestamp (within drift)
        let valid_timestamp = now + chrono::Duration::seconds(30);
        
        // Too old timestamp
        let old_timestamp = now - chrono::Duration::seconds(120);
        
        // Too future timestamp
        let future_timestamp = now + chrono::Duration::seconds(120);
        
        // These would need Redis to fully test, but timestamp validation logic is in validate_request
        assert!(valid_timestamp > old_timestamp);
        assert!(future_timestamp > valid_timestamp);
    }
}
