use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockRequest {
    pub method: String,
    pub path: String,
    /// Optional JSON body pattern to match (subset match)
    pub body_contains: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockResponse {
    pub status: u16,
    pub body: Value,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockEntry {
    pub id: String,
    pub request: MockRequest,
    pub response: MockResponse,
    pub hit_count: u64,
}

#[derive(Clone)]
pub struct MockRegistry {
    entries: Arc<RwLock<Vec<MockEntry>>>,
}

impl MockRegistry {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn register(&self, request: MockRequest, response: MockResponse) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let entry = MockEntry {
            id: id.clone(),
            request,
            response,
            hit_count: 0,
        };
        self.entries.write().await.push(entry);
        id
    }

    pub async fn remove(&self, id: &str) {
        self.entries.write().await.retain(|e| e.id != id);
    }

    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }

    /// Returns the first matching response and increments its hit count.
    pub async fn match_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Option<MockResponse> {
        let mut entries = self.entries.write().await;
        for entry in entries.iter_mut() {
            if entry.request.method.eq_ignore_ascii_case(method)
                && entry.request.path == path
                && body_matches(body, entry.request.body_contains.as_ref())
            {
                entry.hit_count += 1;
                return Some(entry.response.clone());
            }
        }
        None
    }

    pub async fn list(&self) -> Vec<MockEntry> {
        self.entries.read().await.clone()
    }
}

impl Default for MockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true if `actual` contains all keys/values present in `pattern`.
fn body_matches(actual: Option<&Value>, pattern: Option<&Value>) -> bool {
    match (actual, pattern) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(a), Some(p)) => json_subset(a, p),
    }
}

fn json_subset(actual: &Value, pattern: &Value) -> bool {
    match (actual, pattern) {
        (Value::Object(a), Value::Object(p)) => {
            p.iter().all(|(k, v)| a.get(k).map_or(false, |av| json_subset(av, v)))
        }
        _ => actual == pattern,
    }
}
