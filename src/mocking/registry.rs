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
        self.match_http_request(method, path, body, None).await
    }

    /// Match request using method, path, body, and optional headers.
    /// Path supports:
    /// - exact matches
    /// - wildcard segments, e.g. `/creators/*/tips`
    /// - path params, e.g. `/creators/:username/tips`
    /// Query constraints can be included in the registered path:
    /// `/tips?status=settled&network=testnet`.
    pub async fn match_http_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        headers: Option<&HashMap<String, String>>,
    ) -> Option<MockResponse> {
        self.match_http_request_with_context(method, path, body, headers)
            .await
            .map(|(response, _)| response)
    }

    pub async fn match_http_request_with_context(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        headers: Option<&HashMap<String, String>>,
    ) -> Option<(MockResponse, HashMap<String, String>)> {
        let mut entries = self.entries.write().await;
        for entry in entries.iter_mut() {
            if entry.request.method.eq_ignore_ascii_case(method)
                && path_matches(&entry.request.path, path)
                && body_matches(body, entry.request.body_contains.as_ref())
                && headers_match(headers, &entry.response.headers)
            {
                entry.hit_count += 1;
                let params = extract_path_params(&entry.request.path, path);
                return Some((entry.response.clone(), params));
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

/// Header matching is opt-in. We match only `x-mock-match-*` response headers as
/// constraints so existing tests and fixtures remain backwards compatible.
fn headers_match(
    actual: Option<&HashMap<String, String>>,
    response_headers: &HashMap<String, String>,
) -> bool {
    let Some(actual) = actual else {
        return !response_headers
            .keys()
            .any(|k| k.to_ascii_lowercase().starts_with("x-mock-match-"));
    };

    response_headers
        .iter()
        .filter_map(|(k, v)| {
            let lower = k.to_ascii_lowercase();
            lower
                .strip_prefix("x-mock-match-")
                .map(|header_name| (header_name.to_string(), v))
        })
        .all(|(header_name, expected)| {
            actual
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&header_name))
                .map(|(_, v)| v == expected)
                .unwrap_or(false)
        })
}

fn path_matches(registered: &str, incoming: &str) -> bool {
    let (registered_path, registered_query) = split_path_query(registered);
    let (incoming_path, incoming_query) = split_path_query(incoming);

    segment_match(&registered_path, &incoming_path)
        && query_subset_match(&registered_query, &incoming_query)
}

fn split_path_query(input: &str) -> (String, HashMap<String, String>) {
    let mut parts = input.splitn(2, '?');
    let path = parts.next().unwrap_or_default().to_string();
    let query = parts
        .next()
        .map(parse_query_string)
        .unwrap_or_else(HashMap::new);
    (path, query)
}

fn parse_query_string(raw: &str) -> HashMap<String, String> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?.trim();
            let v = parts.next().unwrap_or("").trim();
            if k.is_empty() {
                None
            } else {
                Some((k.to_string(), v.to_string()))
            }
        })
        .collect()
}

fn query_subset_match(
    expected: &HashMap<String, String>,
    incoming: &HashMap<String, String>,
) -> bool {
    expected
        .iter()
        .all(|(k, v)| incoming.get(k).map(|iv| iv == v).unwrap_or(false))
}

fn segment_match(expected_path: &str, incoming_path: &str) -> bool {
    let expected = normalize_path_segments(expected_path);
    let incoming = normalize_path_segments(incoming_path);

    if expected.len() != incoming.len() {
        return false;
    }

    expected
        .iter()
        .zip(incoming.iter())
        .all(|(e, i)| e == "*" || e.starts_with(':') || e == i)
}

fn normalize_path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn extract_path_params(expected_path: &str, incoming_path: &str) -> HashMap<String, String> {
    let expected = normalize_path_segments(expected_path);
    let incoming = normalize_path_segments(incoming_path);
    let mut params = HashMap::new();

    for (e, i) in expected.iter().zip(incoming.iter()) {
        if let Some(name) = e.strip_prefix(':') {
            params.insert(format!("request.path_param.{name}"), i.clone());
        }
    }
    params
}

fn json_subset(actual: &Value, pattern: &Value) -> bool {
    match (actual, pattern) {
        (Value::Object(a), Value::Object(p)) => p
            .iter()
            .all(|(k, v)| a.get(k).map_or(false, |av| json_subset(av, v))),
        _ => actual == pattern,
    }
}
