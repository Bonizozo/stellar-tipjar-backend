use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use tokio::sync::RwLock;

use crate::cache::policies::{CacheEntry, EvictionPolicy};

#[derive(Clone)]
struct StoredValue {
    payload: String,
    created_at: chrono::DateTime<chrono::Utc>,
    last_accessed: chrono::DateTime<chrono::Utc>,
    access_count: u64,
    ttl: ChronoDuration,
}

#[derive(Clone)]
struct SharedLayer {
    store: Arc<RwLock<HashMap<String, StoredValue>>>,
    default_ttl: Duration,
    policy: EvictionPolicy,
}

impl SharedLayer {
    fn new(default_ttl: Duration, policy: EvictionPolicy) -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            default_ttl,
            policy,
        }
    }

    fn default_ttl(&self) -> Duration {
        self.default_ttl
    }

    async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut store = self.store.write().await;

        if let Some(entry) = store.get_mut(key) {
            let expired = entry.created_at + entry.ttl < Utc::now();
            if expired {
                store.remove(key);
                return Ok(None);
            }

            entry.last_accessed = Utc::now();
            entry.access_count = entry.access_count.saturating_add(1);
            return Ok(Some(entry.payload.clone()));
        }

        Ok(None)
    }

    async fn set_with_ttl(&self, key: &str, value: String, ttl: Duration) -> Result<()> {
        let ttl = ChronoDuration::from_std(ttl)
            .unwrap_or_else(|_| ChronoDuration::seconds(self.default_ttl.as_secs() as i64));

        let mut store = self.store.write().await;

        if store.len() >= self.policy.max_entries {
            let entries = store
                .iter()
                .map(|(k, v)| CacheEntry {
                    key: k.clone(),
                    created_at: v.created_at,
                    last_accessed: v.last_accessed,
                    access_count: v.access_count,
                    ttl: v.ttl,
                    size_bytes: v.payload.len(),
                })
                .collect::<Vec<_>>();

            for victim in self.policy.select_victims(&entries, 1) {
                store.remove(&victim);
            }
        }

        let now = Utc::now();
        store.insert(
            key.to_string(),
            StoredValue {
                payload: value,
                created_at: now,
                last_accessed: now,
                access_count: 1,
                ttl,
            },
        );

        Ok(())
    }

    async fn delete_pattern(&self, pattern: &str) -> Result<usize> {
        let mut store = self.store.write().await;
        let matching = store
            .keys()
            .filter(|k| wildcard_match(pattern, k))
            .cloned()
            .collect::<Vec<_>>();

        for key in &matching {
            store.remove(key);
        }

        Ok(matching.len())
    }

    async fn metadata_snapshot(&self) -> Vec<CacheEntry> {
        let store = self.store.read().await;
        store
            .iter()
            .map(|(k, v)| CacheEntry {
                key: k.clone(),
                created_at: v.created_at,
                last_accessed: v.last_accessed,
                access_count: v.access_count,
                ttl: v.ttl,
                size_bytes: v.payload.len(),
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Clone)]
pub struct LocalCache {
    inner: SharedLayer,
}

impl Default for LocalCache {
    fn default() -> Self {
        Self {
            inner: SharedLayer::new(Duration::from_secs(60), EvictionPolicy::default()),
        }
    }
}

impl LocalCache {
    pub fn new(default_ttl: Duration, policy: EvictionPolicy) -> Self {
        Self {
            inner: SharedLayer::new(default_ttl, policy),
        }
    }

    pub fn default_ttl(&self) -> Duration {
        self.inner.default_ttl()
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        self.inner.get(key).await
    }

    pub async fn set_with_ttl(&self, key: &str, value: String, ttl: Duration) -> Result<()> {
        self.inner.set_with_ttl(key, value, ttl).await
    }

    pub async fn delete_pattern(&self, pattern: &str) -> Result<usize> {
        self.inner.delete_pattern(pattern).await
    }

    pub async fn metadata_snapshot(&self) -> Vec<CacheEntry> {
        self.inner.metadata_snapshot().await
    }
}

#[derive(Clone)]
pub struct RedisCache {
    inner: SharedLayer,
}

impl Default for RedisCache {
    fn default() -> Self {
        Self {
            inner: SharedLayer::new(Duration::from_secs(300), EvictionPolicy::default()),
        }
    }
}

impl RedisCache {
    pub fn new(default_ttl: Duration, policy: EvictionPolicy) -> Self {
        Self {
            inner: SharedLayer::new(default_ttl, policy),
        }
    }

    pub fn default_ttl(&self) -> Duration {
        self.inner.default_ttl()
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        self.inner.get(key).await
    }

    pub async fn set_with_ttl(&self, key: &str, value: String, ttl: Duration) -> Result<()> {
        self.inner.set_with_ttl(key, value, ttl).await
    }

    pub async fn delete_pattern(&self, pattern: &str) -> Result<usize> {
        self.inner.delete_pattern(pattern).await
    }
}

#[derive(Clone)]
pub struct DatabaseCache {
    inner: SharedLayer,
}

impl Default for DatabaseCache {
    fn default() -> Self {
        Self {
            inner: SharedLayer::new(Duration::from_secs(1800), EvictionPolicy::default()),
        }
    }
}

impl DatabaseCache {
    pub fn new(default_ttl: Duration, policy: EvictionPolicy) -> Self {
        Self {
            inner: SharedLayer::new(default_ttl, policy),
        }
    }

    pub fn default_ttl(&self) -> Duration {
        self.inner.default_ttl()
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        self.inner.get(key).await
    }

    pub async fn set_with_ttl(&self, key: &str, value: String, ttl: Duration) -> Result<()> {
        self.inner.set_with_ttl(key, value, ttl).await
    }

    pub async fn delete_pattern(&self, pattern: &str) -> Result<usize> {
        self.inner.delete_pattern(pattern).await
    }
}

fn wildcard_match(pattern: &str, input: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if let Some((prefix, suffix)) = pattern.split_once('*') {
        return input.starts_with(prefix) && input.ends_with(suffix);
    }

    pattern == input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pattern_deletes_matching_keys() {
        let layer = LocalCache::default();
        layer
            .set_with_ttl(
                "creator:alice",
                "{\"ok\":true}".to_string(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        layer
            .set_with_ttl(
                "creator:bob",
                "{\"ok\":true}".to_string(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        layer
            .set_with_ttl(
                "tip:alice",
                "{\"ok\":true}".to_string(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();

        let removed = layer.delete_pattern("creator:*").await.unwrap();
        assert_eq!(removed, 2);
        assert!(layer.get("tip:alice").await.unwrap().is_some());
        assert!(layer.get("creator:alice").await.unwrap().is_none());
    }
}
