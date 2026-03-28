use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};

use crate::cache::MultiLayerCache;

const INVALIDATION_CHANNEL: &str = "cache:invalidation";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidationMessage {
    pub pattern: String,
    pub node_id: String,
    pub timestamp: DateTime<Utc>,
}

#[async_trait::async_trait]
pub trait CoordinationBus: Send + Sync {
    async fn publish(&self, channel: &str, payload: String) -> Result<()>;
    async fn subscribe(&self, channel: &str) -> Result<mpsc::Receiver<String>>;
}

#[derive(Default)]
pub struct InMemoryCoordinationBus {
    subscribers: Mutex<HashMap<String, Vec<mpsc::Sender<String>>>>,
}

#[async_trait::async_trait]
impl CoordinationBus for InMemoryCoordinationBus {
    async fn publish(&self, channel: &str, payload: String) -> Result<()> {
        let mut guard = self.subscribers.lock().await;
        if let Some(list) = guard.get_mut(channel) {
            list.retain(|tx| !tx.is_closed());
            for tx in list.iter() {
                let _ = tx.send(payload.clone()).await;
            }
        }
        Ok(())
    }

    async fn subscribe(&self, channel: &str) -> Result<mpsc::Receiver<String>> {
        let (tx, rx) = mpsc::channel(128);
        let mut guard = self.subscribers.lock().await;
        guard.entry(channel.to_string()).or_default().push(tx);
        Ok(rx)
    }
}

pub struct CacheCoordinator {
    bus: Arc<dyn CoordinationBus>,
    pub node_id: String,
}

impl CacheCoordinator {
    pub fn new(bus: Arc<dyn CoordinationBus>, node_id: impl Into<String>) -> Self {
        Self {
            bus,
            node_id: node_id.into(),
        }
    }

    pub async fn coordinate_invalidation(&self, pattern: &str) -> Result<()> {
        let message = serde_json::to_string(&InvalidationMessage {
            pattern: pattern.to_string(),
            node_id: self.node_id.clone(),
            timestamp: Utc::now(),
        })?;

        self.bus.publish(INVALIDATION_CHANNEL, message).await
    }

    pub async fn listen_for_invalidations(&self, cache: Arc<MultiLayerCache>) -> Result<()> {
        let mut subscription = self.bus.subscribe(INVALIDATION_CHANNEL).await?;

        while let Some(payload) = subscription.recv().await {
            let msg = match serde_json::from_str::<InvalidationMessage>(&payload) {
                Ok(msg) => msg,
                Err(err) => {
                    tracing::warn!(error = %err, "invalid cache invalidation payload");
                    continue;
                }
            };

            if msg.node_id == self.node_id {
                continue;
            }

            if let Err(err) = cache.invalidate_l1_pattern(&msg.pattern).await {
                tracing::warn!(error = %err, pattern = %msg.pattern, "failed to apply remote invalidation");
            }
        }

        Err(anyhow!("invalidation listener stopped unexpectedly"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::layers::{DatabaseCache, LocalCache, RedisCache};

    #[tokio::test]
    async fn peer_invalidation_clears_l1() {
        let bus = Arc::new(InMemoryCoordinationBus::default());
        let sender = CacheCoordinator::new(bus.clone(), "node-a");
        let receiver = CacheCoordinator::new(bus, "node-b");

        let cache = Arc::new(MultiLayerCache::new(
            Arc::new(LocalCache::default()),
            Arc::new(RedisCache::default()),
            Arc::new(DatabaseCache::default()),
        ));

        cache
            .set(
                "creator:alice",
                &serde_json::json!({"rank": 1}),
                std::time::Duration::from_secs(30),
            )
            .await
            .unwrap();

        let cloned_cache = Arc::clone(&cache);
        let listener = tokio::spawn(async move {
            let _ = receiver.listen_for_invalidations(cloned_cache).await;
        });

        sender.coordinate_invalidation("creator:*").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(cache.l1.get("creator:alice").await.unwrap().is_none());

        listener.abort();
    }
}
