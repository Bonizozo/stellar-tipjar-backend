use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedInteraction {
    pub id: String,
    pub method: String,
    pub path: String,
    pub request_body: Option<Value>,
    pub response_status: u16,
    pub response_body: Value,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct MockRecorder {
    interactions: Arc<RwLock<Vec<RecordedInteraction>>>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl MockRecorder {
    pub fn new() -> Self {
        Self {
            interactions: Arc::new(RwLock::new(Vec::new())),
            enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn enable(&self) {
        self.enabled.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn disable(&self) {
        self.enabled.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn record(
        &self,
        method: &str,
        path: &str,
        request_body: Option<Value>,
        response_status: u16,
        response_body: Value,
    ) {
        if !self.is_enabled() {
            return;
        }
        let interaction = RecordedInteraction {
            id: uuid::Uuid::new_v4().to_string(),
            method: method.to_string(),
            path: path.to_string(),
            request_body,
            response_status,
            response_body,
            recorded_at: Utc::now(),
        };
        self.interactions.write().await.push(interaction);
    }

    pub async fn get_all(&self) -> Vec<RecordedInteraction> {
        self.interactions.read().await.clone()
    }

    pub async fn clear(&self) {
        self.interactions.write().await.clear();
    }
}

impl Default for MockRecorder {
    fn default() -> Self {
        Self::new()
    }
}
