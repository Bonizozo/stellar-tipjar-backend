use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;

use crate::cache::MultiLayerCache;
use crate::models::creator::Creator;

#[async_trait::async_trait]
pub trait WarmableDataSource<T>: Send + Sync
where
    T: serde::Serialize + Clone + Send + Sync + 'static,
{
    async fn fetch_popular(&self, limit: i64) -> Result<Vec<T>>;
    fn cache_key(&self, item: &T) -> String;
}

#[derive(Clone)]
pub struct CreatorWarmSource {
    pub pool: PgPool,
}

#[async_trait::async_trait]
impl WarmableDataSource<Creator> for CreatorWarmSource {
    async fn fetch_popular(&self, limit: i64) -> Result<Vec<Creator>> {
        let creators = sqlx::query_as::<_, Creator>(
            r#"
            SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at
            FROM creators
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(creators)
    }

    fn cache_key(&self, item: &Creator) -> String {
        format!("creator:{}", item.username)
    }
}

pub struct CacheWarmer<T, S>
where
    T: serde::Serialize + Clone + Send + Sync + 'static,
    S: WarmableDataSource<T>,
{
    cache: Arc<MultiLayerCache>,
    source: Arc<S>,
    ttl: Duration,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, S> CacheWarmer<T, S>
where
    T: serde::Serialize + Clone + Send + Sync + 'static,
    S: WarmableDataSource<T>,
{
    pub fn new(cache: Arc<MultiLayerCache>, source: Arc<S>, ttl: Duration) -> Self {
        Self {
            cache,
            source,
            ttl,
            _phantom: std::marker::PhantomData,
        }
    }

    pub async fn warm_popular(&self, limit: i64) -> Result<usize> {
        let items = self.source.fetch_popular(limit).await?;

        for item in &items {
            let key = self.source.cache_key(item);
            self.cache.set(&key, item, self.ttl).await?;
        }

        Ok(items.len())
    }

    pub async fn warm_on_schedule(self: Arc<Self>, every: Duration, limit: i64) {
        let mut interval = tokio::time::interval(every);

        loop {
            interval.tick().await;
            if let Err(err) = self.warm_popular(limit).await {
                tracing::error!(error = %err, "cache warming cycle failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct FakeCreatorSource {
        creators: Mutex<Vec<Creator>>,
    }

    #[async_trait::async_trait]
    impl WarmableDataSource<Creator> for FakeCreatorSource {
        async fn fetch_popular(&self, _limit: i64) -> Result<Vec<Creator>> {
            Ok(self.creators.lock().await.clone())
        }

        fn cache_key(&self, item: &Creator) -> String {
            format!("creator:{}", item.username)
        }
    }

    #[tokio::test]
    async fn warmer_populates_cache() {
        let cache = Arc::new(MultiLayerCache::with_defaults());

        let source = Arc::new(FakeCreatorSource::default());
        source.creators.lock().await.push(Creator {
            id: Uuid::new_v4(),
            username: "alice".to_string(),
            wallet_address: "GABC...".to_string(),
            email: None,
            created_at: Utc::now(),
        });

        let warmer = CacheWarmer::new(cache.clone(), source, Duration::from_secs(120));
        let warmed = warmer.warm_popular(10).await.unwrap();
        assert_eq!(warmed, 1);

        let cached: Option<Creator> = cache.get("creator:alice").await.unwrap();
        assert!(cached.is_some());
    }
}
