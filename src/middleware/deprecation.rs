use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

const SUNSET_DATE: &str = "Sat, 01 Jan 2027 00:00:00 GMT";
const MIGRATION_LINK: &str =
    r#"<https://docs.example.com/migration/v1-to-v2>; rel="deprecation""#;

/// Injects `Deprecation`, `Sunset`, and `Link` headers on v1 responses.
pub async fn deprecation_notice(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("Deprecation", HeaderValue::from_static("true"));
    headers.insert(
        "Link",
        HeaderValue::from_static(r#"</api/v2>; rel="successor-version""#),
    );
    headers.insert("Sunset", HeaderValue::from_static(SUNSET_DATE));
    response
}

/// Per-path hit counter for deprecated endpoints.
#[derive(Default)]
pub struct DeprecationTracker {
    counts: Arc<RwLock<HashMap<String, AtomicU64>>>,
}

impl DeprecationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record(&self, path: &str) {
        let mut map = self.counts.write().await;
        map.entry(path.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub async fn snapshot(&self) -> HashMap<String, u64> {
        self.counts
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
            .collect()
    }
}

/// Middleware that records deprecated-endpoint usage and injects headers.
/// Attach this only to v1 routes.
pub async fn track_deprecated_usage(
    tracker: Arc<DeprecationTracker>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    tracker.record(&path).await;

    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("Deprecation", HeaderValue::from_static("true"));
    headers.insert("Sunset", HeaderValue::from_static(SUNSET_DATE));
    headers.insert("Link", HeaderValue::from_static(MIGRATION_LINK));
    headers.insert(
        "X-Deprecation-Warning",
        HeaderValue::from_static("This API version is deprecated. Please migrate to /api/v2"),
    );
    response
}
