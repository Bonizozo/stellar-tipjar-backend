pub mod coordination;
pub mod invalidation;
pub mod keys;
pub mod layers;
pub mod policies;
pub mod redis_client;
pub mod warming;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::http::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use redis::aio::ConnectionManager;

pub use self::coordination::{CacheCoordinator, InMemoryCoordinationBus, InvalidationMessage};
pub use self::invalidation::{CacheInvalidator, InvalidationEvent, InvalidationPublisher};
pub use self::layers::{DatabaseCache, LocalCache, RedisCache};
pub use self::policies::{CacheEntry, EvictionPolicy, EvictionStrategy};
pub use self::warming::{CacheWarmer, CreatorWarmSource, WarmableDataSource};

/// Serializable cached HTTP response for middleware-level caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedHttpResponse {
    pub status: u16,
    pub headers: HashMap<String, Vec<String>>,
    pub body: Vec<u8>,
    pub cached_at: chrono::DateTime<chrono::Utc>,
}

impl CachedHttpResponse {
    pub fn new(status: StatusCode, headers: HashMap<String, Vec<String>>, body: Vec<u8>) -> Self {
        Self {
            status: status.as_u16(),
            headers,
            body,
            cached_at: chrono::Utc::now(),
        }
    }

    pub fn status(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK)
    }
}

#[derive(Clone)]
pub struct MultiLayerCache {
    pub l1: Arc<LocalCache>,
    pub l2: Arc<RedisCache>,
    pub l3: Arc<DatabaseCache>,
}

impl MultiLayerCache {
    pub fn new(l1: Arc<LocalCache>, l2: Arc<RedisCache>, l3: Arc<DatabaseCache>) -> Self {
        Self { l1, l2, l3 }
    }

    pub fn with_defaults() -> Self {
        Self {
            l1: Arc::new(LocalCache::default()),
            l2: Arc::new(RedisCache::default()),
            l3: Arc::new(DatabaseCache::default()),
        }
    }

    pub async fn get<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned + Serialize + Clone,
    {
        if let Some(raw) = self.l1.get(key).await? {
            return Ok(serde_json::from_str(&raw).ok());
        }

        if let Some(raw) = self.l2.get(key).await? {
            self.l1
                .set_with_ttl(key, raw.clone(), self.l1.default_ttl())
                .await?;
            return Ok(serde_json::from_str(&raw).ok());
        }

        if let Some(raw) = self.l3.get(key).await? {
            self.l2
                .set_with_ttl(key, raw.clone(), self.l2.default_ttl())
                .await?;
            self.l1
                .set_with_ttl(key, raw.clone(), self.l1.default_ttl())
                .await?;
            return Ok(serde_json::from_str(&raw).ok());
        }

        Ok(None)
    }

    pub async fn set<T>(&self, key: &str, value: &T, ttl: Duration) -> Result<()>
    where
        T: Serialize,
    {
        let raw = serde_json::to_string(value)?;
        self.l1.set_with_ttl(key, raw.clone(), ttl).await?;
        self.l2.set_with_ttl(key, raw.clone(), ttl).await?;
        self.l3.set_with_ttl(key, raw, ttl).await?;
        Ok(())
    }

    pub async fn invalidate_pattern(&self, pattern: &str) -> Result<()> {
        let _ = self.l1.delete_pattern(pattern).await?;
        let _ = self.l2.delete_pattern(pattern).await?;
        let _ = self.l3.delete_pattern(pattern).await?;
        Ok(())
    }

    pub async fn invalidate_l1_pattern(&self, pattern: &str) -> Result<()> {
        let _ = self.l1.delete_pattern(pattern).await?;
        Ok(())
    }

    /// Get a cached HTTP response (shortcut for middleware usage).
    pub async fn get_http_response(&self, key: &str) -> Result<Option<CachedHttpResponse>> {
        self.get::<CachedHttpResponse>(key).await
    }

    /// Store an HTTP response in all layers.
    pub async fn set_http_response(
        &self,
        key: &str,
        response: &CachedHttpResponse,
        ttl: Duration,
    ) -> Result<()> {
        self.set(key, response, ttl).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_from_lower_tier_and_backfills_upper_layers() {
        let cache = MultiLayerCache::with_defaults();
        let payload = serde_json::json!({"v": 7});

        cache
            .l3
            .set_with_ttl(
                "creator:alice",
                payload.to_string(),
                Duration::from_secs(30),
            )
            .await
            .unwrap();

        let value: Option<serde_json::Value> = cache.get("creator:alice").await.unwrap();
        assert_eq!(value, Some(payload));

        let l2 = cache.l2.get("creator:alice").await.unwrap();
        let l1 = cache.l1.get("creator:alice").await.unwrap();
        assert!(l2.is_some());
        assert!(l1.is_some());
    }
}
