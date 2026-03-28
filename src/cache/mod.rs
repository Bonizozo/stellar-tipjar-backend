pub mod coordination;
pub mod invalidation;
pub mod keys;
pub mod layers;
pub mod policies;
pub mod redis_client;
pub mod warming;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub use redis::aio::ConnectionManager;

pub use self::coordination::{CacheCoordinator, InMemoryCoordinationBus, InvalidationMessage};
pub use self::invalidation::{CacheInvalidator, InvalidationEvent, InvalidationPublisher};
pub use self::layers::{DatabaseCache, LocalCache, RedisCache};
pub use self::policies::{CacheEntry, EvictionPolicy, EvictionStrategy};
pub use self::warming::{CacheWarmer, CreatorWarmSource, WarmableDataSource};

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
