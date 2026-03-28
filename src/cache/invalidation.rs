use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::cache::coordination::CacheCoordinator;
use crate::cache::MultiLayerCache;

#[derive(Debug, Clone)]
pub struct InvalidationEvent {
    pub pattern: String,
    pub timestamp: DateTime<Utc>,
}

#[async_trait::async_trait]
pub trait InvalidationPublisher: Send + Sync {
    async fn publish(&self, event: InvalidationEvent) -> Result<()>;
}

#[async_trait::async_trait]
impl InvalidationPublisher for CacheCoordinator {
    async fn publish(&self, event: InvalidationEvent) -> Result<()> {
        self.coordinate_invalidation(&event.pattern).await
    }
}

pub struct CacheInvalidator {
    cache: Arc<MultiLayerCache>,
    publisher: Option<Arc<dyn InvalidationPublisher>>,
}

impl CacheInvalidator {
    pub fn new(
        cache: Arc<MultiLayerCache>,
        publisher: Option<Arc<dyn InvalidationPublisher>>,
    ) -> Self {
        Self { cache, publisher }
    }

    pub async fn invalidate_pattern(&self, pattern: &str) -> Result<()> {
        self.cache.invalidate_pattern(pattern).await?;

        if let Some(publisher) = &self.publisher {
            publisher
                .publish(InvalidationEvent {
                    pattern: pattern.to_string(),
                    timestamp: Utc::now(),
                })
                .await?;
        }

        Ok(())
    }

    pub async fn invalidate_on_update(&self, entity: &str, id: &Uuid) -> Result<()> {
        let id_pattern = format!("{}:{}", entity, id);
        let entity_pattern = format!("{}:*", entity);
        let list_pattern = format!("list:{}:*", entity);

        for pattern in [id_pattern, entity_pattern, list_pattern] {
            self.invalidate_pattern(&pattern).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::layers::{DatabaseCache, LocalCache, RedisCache};

    #[tokio::test]
    async fn invalidator_removes_matching_keys() {
        let cache = Arc::new(MultiLayerCache::new(
            Arc::new(LocalCache::default()),
            Arc::new(RedisCache::default()),
            Arc::new(DatabaseCache::default()),
        ));

        cache
            .set(
                "creator:alice",
                &serde_json::json!({"id": 1}),
                std::time::Duration::from_secs(60),
            )
            .await
            .unwrap();

        let invalidator = CacheInvalidator::new(Arc::clone(&cache), None);
        invalidator.invalidate_pattern("creator:*").await.unwrap();

        let value: Option<serde_json::Value> = cache.get("creator:alice").await.unwrap();
        assert!(value.is_none());
    }
}
