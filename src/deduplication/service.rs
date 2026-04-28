use anyhow::Result;
use chrono::{DateTime, Utc, Duration};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

use super::fingerprint::{RequestFingerprint, FingerprintConfig, FingerprintGenerator};

#[derive(Error, Debug)]
pub enum DeduplicationError {
    #[error("Request already processed: {0}")]
    RequestAlreadyProcessed(String),
    #[error("Redis operation failed: {0}")]
    RedisError(String),
    #[error("Invalid fingerprint: {0}")]
    InvalidFingerprint(String),
    #[error("Storage error: {0}")]
    StorageError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeduplicationConfig {
    pub default_ttl_seconds: u64,
    pub idempotent_ttl_seconds: u64,
    pub cleanup_interval_seconds: u64,
    pub max_stored_requests: usize,
    pub enabled_endpoints: Vec<String>,
    pub fingerprint_config: FingerprintConfig,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            default_ttl_seconds: 300, // 5 minutes
            idempotent_ttl_seconds: 3600, // 1 hour for idempotent requests
            cleanup_interval_seconds: 600, // 10 minutes
            max_stored_requests: 10000,
            enabled_endpoints: vec![
                "/api/v1/tips".to_string(),
                "/api/v1/withdrawals".to_string(),
                "/api/v1/transfers".to_string(),
            ],
            fingerprint_config: FingerprintConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    pub fingerprint: RequestFingerprint,
    pub processed_at: DateTime<Utc>,
    pub response_status: u16,
    pub response_body_hash: Option<String>,
    pub processing_time_ms: u64,
    pub client_id: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl RequestRecord {
    pub fn new(
        fingerprint: RequestFingerprint,
        response_status: u16,
        processing_time_ms: u64,
        client_id: Option<String>,
    ) -> Self {
        Self {
            fingerprint,
            processed_at: Utc::now(),
            response_status,
            response_body_hash: None,
            processing_time_ms,
            client_id,
            metadata: HashMap::new(),
        }
    }

    pub fn with_response_body(mut self, body: &str) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        self.response_body_hash = Some(format!("{:x}", hasher.finalize()));
        self
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    pub fn is_expired(&self, ttl_seconds: u64) -> bool {
        let expiry_time = self.processed_at + Duration::seconds(ttl_seconds as i64);
        Utc::now() > expiry_time
    }
}

pub struct DeduplicationService {
    redis: Option<Arc<ConnectionManager>>,
    config: DeduplicationConfig,
    fingerprint_generator: FingerprintGenerator,
    local_cache: Arc<RwLock<HashMap<String, RequestRecord>>>,
}

impl DeduplicationService {
    pub fn new(redis: Option<Arc<ConnectionManager>>, config: DeduplicationConfig) -> Self {
        let fingerprint_generator = FingerprintGenerator::new(config.fingerprint_config.clone());
        
        Self {
            redis,
            config,
            fingerprint_generator,
            local_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_redis(redis: Arc<ConnectionManager>) -> Self {
        Self::new(Some(redis), DeduplicationConfig::default())
    }

    pub fn without_redis() -> Self {
        Self::new(None, DeduplicationConfig::default())
    }

    /// Generate a fingerprint for a request
    pub fn generate_fingerprint(
        &self,
        method: &str,
        path: &str,
        body: &str,
        headers: &HashMap<String, String>,
        query_params: &HashMap<String, String>,
        client_id: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> RequestFingerprint {
        self.fingerprint_generator.generate_fingerprint(
            method,
            path,
            body,
            headers,
            query_params,
            client_id,
            idempotency_key,
        )
    }

    /// Check if a request has been processed
    pub async fn is_request_processed(&self, fingerprint: &RequestFingerprint) -> Result<Option<RequestRecord>, DeduplicationError> {
        // Check if deduplication is enabled for this endpoint
        if !self.config.enabled_endpoints.contains(&fingerprint.path) {
            return Ok(None);
        }

        // Check Redis first if available
        if let Some(redis) = &self.redis {
            if let Some(record) = self.check_redis(fingerprint).await? {
                return Ok(Some(record));
            }
        }

        // Fallback to local cache
        self.check_local_cache(fingerprint).await
    }

    async fn check_redis(&self, fingerprint: &RequestFingerprint) -> Result<Option<RequestRecord>, DeduplicationError> {
        let redis = self.redis.as_ref().unwrap();
        let mut conn = redis.clone();
        
        let key = format!("dedup:{}", fingerprint.hash);

        let value: Option<String> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

        if let Some(value) = value {
            let record: RequestRecord = serde_json::from_str(&value)
                .map_err(|e| DeduplicationError::InvalidFingerprint(e.to_string()))?;

            // Check if record is expired
            let ttl = if fingerprint.is_idempotent_request() {
                self.config.idempotent_ttl_seconds
            } else {
                self.config.default_ttl_seconds
            };

            if record.is_expired(ttl) {
                // Clean up expired record
                redis::cmd("DEL")
                    .arg(&key)
                    .query_async::<_, ()>(&mut conn)
                    .await
                    .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;
                return Ok(None);
            }

            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn check_local_cache(&self, fingerprint: &RequestFingerprint) -> Result<Option<RequestRecord>, DeduplicationError> {
        let cache = self.local_cache.read().await;
        
        if let Some(record) = cache.get(&fingerprint.hash) {
            let ttl = if fingerprint.is_idempotent_request() {
                self.config.idempotent_ttl_seconds
            } else {
                self.config.default_ttl_seconds
            };

            if record.is_expired(ttl) {
                return Ok(None);
            }

            Ok(Some(record.clone()))
        } else {
            Ok(None)
        }
    }

    /// Record a processed request
    pub async fn record_request(&self, record: RequestRecord) -> Result<(), DeduplicationError> {
        // Check if deduplication is enabled for this endpoint
        if !self.config.enabled_endpoints.contains(&record.fingerprint.path) {
            return Ok(());
        }

        // Store in Redis if available
        if let Some(redis) = &self.redis {
            self.store_in_redis(&record).await?;
        }

        // Also store in local cache as backup
        self.store_in_local_cache(&record).await;

        Ok(())
    }

    async fn store_in_redis(&self, record: &RequestRecord) -> Result<(), DeduplicationError> {
        let redis = self.redis.as_ref().unwrap();
        let mut conn = redis.clone();
        
        let key = format!("dedup:{}", record.fingerprint.hash);
        let serialized = serde_json::to_string(record)
            .map_err(|e| DeduplicationError::StorageError(e.to_string()))?;

        let ttl = if record.fingerprint.is_idempotent_request() {
            self.config.idempotent_ttl_seconds
        } else {
            self.config.default_ttl_seconds
        };

        redis::cmd("SETEX")
            .arg(&key)
            .arg(ttl)
            .arg(&serialized)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

        Ok(())
    }

    async fn store_in_local_cache(&self, record: &RequestRecord) -> Result<(), DeduplicationError> {
        let mut cache = self.local_cache.write().await;
        
        cache.insert(record.fingerprint.hash.clone(), record.clone());

        // Limit cache size
        if cache.len() > self.config.max_stored_requests {
            // Remove oldest entries (simple FIFO)
            let keys_to_remove: Vec<String> = cache
                .keys()
                .take(cache.len() - self.config.max_stored_requests)
                .cloned()
                .collect();
            
            for key in keys_to_remove {
                cache.remove(&key);
            }
        }

        Ok(())
    }

    /// Clean up expired records
    pub async fn cleanup_expired_records(&self) -> Result<u64, DeduplicationError> {
        let mut cleaned_count = 0;

        // Clean up Redis
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "dedup:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

            for key in keys {
                if let Ok(Some(record)) = self.get_record_from_redis(&key).await {
                    let ttl = if record.fingerprint.is_idempotent_request() {
                        self.config.idempotent_ttl_seconds
                    } else {
                        self.config.default_ttl_seconds
                    };

                    if record.is_expired(ttl) {
                        redis::cmd("DEL")
                            .arg(&key)
                            .query_async::<_, ()>(&mut conn)
                            .await
                            .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;
                        cleaned_count += 1;
                    }
                }
            }
        }

        // Clean up local cache
        {
            let mut cache = self.local_cache.write().await;
            let mut keys_to_remove = Vec::new();

            for (hash, record) in cache.iter() {
                let ttl = if record.fingerprint.is_idempotent_request() {
                    self.config.idempotent_ttl_seconds
                } else {
                    self.config.default_ttl_seconds
                };

                if record.is_expired(ttl) {
                    keys_to_remove.push(hash.clone());
                }
            }

            for key in keys_to_remove {
                cache.remove(&key);
                cleaned_count += 1;
            }
        }

        Ok(cleaned_count)
    }

    async fn get_record_from_redis(&self, key: &str) -> Result<Option<RequestRecord>, DeduplicationError> {
        let redis = self.redis.as_ref().unwrap();
        let mut conn = redis.clone();

        let value: Option<String> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut conn)
            .await
            .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

        if let Some(value) = value {
            let record: RequestRecord = serde_json::from_str(&value)
                .map_err(|e| DeduplicationError::InvalidFingerprint(e.to_string()))?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// Get deduplication statistics
    pub async fn get_deduplication_stats(&self) -> Result<DeduplicationStats, DeduplicationError> {
        let mut stats = DeduplicationStats::default();

        // Get Redis stats
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "dedup:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

            stats.redis_total_requests = keys.len() as u64;

            // Count by endpoint
            let mut endpoint_counts: HashMap<String, u64> = HashMap::new();
            let mut idempotent_count = 0;

            for key in keys {
                if let Ok(Some(record)) = self.get_record_from_redis(&key).await {
                    *endpoint_counts.entry(record.fingerprint.path.clone()).or_insert(0) += 1;
                    
                    if record.fingerprint.is_idempotent_request() {
                        idempotent_count += 1;
                    }
                }
            }

            stats.requests_by_endpoint = endpoint_counts;
            stats.idempotent_requests = idempotent_count;
        }

        // Get local cache stats
        {
            let cache = self.local_cache.read().await;
            stats.local_cache_requests = cache.len() as u64;
        }

        Ok(stats)
    }

    /// Clear all deduplication records
    pub async fn clear_all_records(&self) -> Result<u64, DeduplicationError> {
        let mut cleared_count = 0;

        // Clear Redis
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "dedup:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

            if !keys.is_empty() {
                redis::cmd("DEL")
                    .arg(keys)
                    .query_async::<_, u64>(&mut conn)
                    .await
                    .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;
                cleared_count += keys.len() as u64;
            }
        }

        // Clear local cache
        {
            let mut cache = self.local_cache.write().await();
            cleared_count += cache.len() as u64;
            cache.clear();
        }

        Ok(cleared_count)
    }

    /// Get records for a specific client
    pub async fn get_client_records(&self, client_id: &str) -> Result<Vec<RequestRecord>, DeduplicationError> {
        let mut records = Vec::new();

        // Get from Redis
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "dedup:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| DeduplicationError::RedisError(e.to_string()))?;

            for key in keys {
                if let Ok(Some(record)) = self.get_record_from_redis(&key).await {
                    if let Some(ref record_client_id) = record.client_id {
                        if record_client_id == client_id {
                            records.push(record);
                        }
                    }
                }
            }
        }

        // Get from local cache
        {
            let cache = self.local_cache.read().await;
            for record in cache.values() {
                if let Some(ref record_client_id) = record.client_id {
                    if record_client_id == client_id {
                        records.push(record.clone());
                    }
                }
            }
        }

        // Sort by processed_at (newest first)
        records.sort_by(|a, b| b.processed_at.cmp(&a.processed_at));

        Ok(records)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeduplicationStats {
    pub redis_total_requests: u64,
    pub local_cache_requests: u64,
    pub requests_by_endpoint: HashMap<String, u64>,
    pub idempotent_requests: u64,
    pub regular_requests: u64,
}

impl DeduplicationStats {
    pub fn total_requests(&self) -> u64 {
        self.redis_total_requests + self.local_cache_requests
    }

    pub fn idempotency_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            (self.idempotent_requests as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_request_record_creation() {
        let fingerprint = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &HashMap::new(),
            &HashMap::new(),
            None,
            None,
        );

        let record = RequestRecord::new(fingerprint, 200, 150, Some("client123".to_string()));

        assert_eq!(record.response_status, 200);
        assert_eq!(record.processing_time_ms, 150);
        assert_eq!(record.client_id, Some("client123".to_string()));
        assert!(!record.is_expired(300)); // 5 minutes
    }

    #[test]
    fn test_request_record_with_response_body() {
        let fingerprint = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &HashMap::new(),
            &HashMap::new(),
            None,
            None,
        );

        let record = RequestRecord::new(fingerprint, 200, 150, None)
            .with_response_body("{\"success\":true}")
            .with_metadata("key".to_string(), "value".to_string());

        assert!(record.response_body_hash.is_some());
        assert_eq!(record.metadata.get("key"), Some(&"value".to_string()));
    }

    #[tokio::test]
    async fn test_deduplication_service_without_redis() {
        let service = DeduplicationService::without_redis();
        
        let fingerprint = service.generate_fingerprint(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &HashMap::new(),
            &HashMap::new(),
            None,
            None,
        );

        // Should return None (not processed) without Redis
        let result = service.is_request_processed(&fingerprint).await.unwrap();
        assert!(result.is_none());

        let record = RequestRecord::new(fingerprint, 200, 150, None);
        service.record_request(record).await.unwrap();

        let stats = service.get_deduplication_stats().await.unwrap();
        assert_eq!(stats.total_requests(), 0); // Redis not available
    }

    #[tokio::test]
    async fn test_deduplication_stats() {
        let stats = DeduplicationStats::default();
        assert_eq!(stats.total_requests(), 0);
        assert_eq!(stats.idempotency_rate(), 0.0);
    }
}
