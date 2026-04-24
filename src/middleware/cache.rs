use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::cache::{keys, CachedHttpResponse, MultiLayerCache};
use crate::db::connection::AppState;

/// Route-aware TTL configuration for intelligent caching.
fn route_ttl(path: &str) -> Duration {
    if path.starts_with("/api/v1/creators/") || path.starts_with("/api/v2/creators/") {
        if path.contains("/tips") {
            Duration::from_secs(60) // 1 minute for tip lists
        } else {
            Duration::from_secs(300) // 5 minutes for creator profiles
        }
    } else if path.contains("/leaderboard/") {
        Duration::from_secs(300) // 5 minutes for leaderboards
    } else if path.contains("/tips") {
        Duration::from_secs(60) // 1 minute for global tips
    } else if path.contains("/search") {
        Duration::from_secs(120) // 2 minutes for search
    } else {
        Duration::from_secs(300) // default 5 minutes
    }
}

/// Generate an ETag from body bytes.
fn generate_etag(body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body);
    let hash = hasher.finalize();
    format!("W/\"{}\"", general_purpose::STANDARD.encode(hash))
}

/// Generate a Last-Modified timestamp from a DateTime.
fn format_http_date(dt: DateTime<Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Parse an HTTP date header value into a DateTime.
fn parse_http_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(value, "%a, %d %b %Y %H:%M:%S GMT")
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Build a cache key from the incoming request.
fn cache_key_from_request(req: &Request<Body>) -> String {
    let method = req.method().as_str();
    let path = req.uri().path();
    let query = req.uri().query().unwrap_or("");
    keys::http_response(method, path, query)
}

/// Intelligent response caching middleware.
///
/// - Caches full GET/HEAD 200 OK responses in the multi-layer cache.
/// - Serves cached responses directly on cache hit (adds `X-Cache-Status: HIT`, `Age`).
/// - Supports conditional requests via `If-None-Match` (ETag) and `If-Modified-Since`.
/// - Adds `Cache-Control`, `ETag`, `Last-Modified`, `Vary`, and `X-Cache-Status: MISS` on fresh responses.
pub async fn intelligent_cache(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Only apply to GET and HEAD
    if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
        return next.run(req).await;
    }

    let cache_key = cache_key_from_request(&req);
    let if_none_match = req.headers().get(header::IF_NONE_MATCH).cloned();
    let if_modified_since = req.headers().get(header::IF_MODIFIED_SINCE).cloned();
    let path = req.uri().path().to_string();

    // Try to serve from cache
    if let Some(cache) = state.cache.as_ref() {
        if let Ok(Some(cached)) = cache.get_http_response(&cache_key).await {
            let age_secs = (Utc::now() - cached.cached_at).num_seconds().max(0) as u64;

            // Conditional request: If-None-Match
            let etag = generate_etag(&cached.body);
            if let Some(ref inm) = if_none_match {
                if inm == etag.as_str() {
                    return Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(header::ETAG, etag)
                        .header(header::CACHE_CONTROL, "public, max-age=3600")
                        .header(header::VARY, "Accept-Encoding")
                        .header("X-Cache-Status", "HIT")
                        .header("Age", age_secs.to_string())
                        .body(Body::empty())
                        .expect("Failed to build 304 response");
                }
            }

            // Conditional request: If-Modified-Since
            if let Some(ref ims) = if_modified_since {
                if let Ok(ims_str) = ims.to_str() {
                    if let Some(ims_dt) = parse_http_date(ims_str) {
                        if cached.cached_at <= ims_dt {
                            return Response::builder()
                                .status(StatusCode::NOT_MODIFIED)
                                .header(header::ETAG, etag)
                                .header(header::LAST_MODIFIED, format_http_date(cached.cached_at))
                                .header(header::CACHE_CONTROL, "public, max-age=3600")
                                .header(header::VARY, "Accept-Encoding")
                                .header("X-Cache-Status", "HIT")
                                .header("Age", age_secs.to_string())
                                .body(Body::empty())
                                .expect("Failed to build 304 response");
                        }
                    }
                }
            }

            // Cache hit — reconstruct response
            let mut builder = Response::builder().status(cached.status());
            for (name, values) in &cached.headers {
                for value in values {
                    builder = builder.header(name.as_str(), value.as_str());
                }
            }
            return builder
                .header(header::ETAG, etag)
                .header(header::LAST_MODIFIED, format_http_date(cached.cached_at))
                .header("X-Cache-Status", "HIT")
                .header("Age", age_secs.to_string())
                .body(Body::from(cached.body.clone()))
                .expect("Failed to build cached response");
        }
    }

    // Cache miss — proceed to handler
    let response = next.run(req).await;

    // Only cache successful 200 OK responses
    if response.status() != StatusCode::OK {
        return response;
    }

    let (mut parts, body) = response.into_parts();

    let body_bytes = match to_bytes(body, 1_000_000).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to collect response body for caching: {}", e);
            return Response::from_parts(parts, Body::empty());
        }
    };

    let etag_value = generate_etag(&body_bytes);
    let now = Utc::now();

    // Store in multi-layer cache
    if let Some(cache) = state.cache.as_ref() {
        let mut headers: HashMap<String, Vec<String>> = HashMap::new();
        for (name, value) in &parts.headers {
            let key = name.as_str().to_string();
            let val = value.to_str().unwrap_or("").to_string();
            headers.entry(key).or_default().push(val);
        }

        let cached_response = CachedHttpResponse::new(
            StatusCode::OK,
            headers,
            body_bytes.to_vec(),
        );

        let ttl = route_ttl(&path);
        if let Err(e) = cache.set_http_response(&cache_key, &cached_response, ttl).await {
            tracing::warn!(error = %e, key = %cache_key, "Failed to cache HTTP response");
        }
    }

    // Insert cache headers on the outgoing response
    parts.headers.insert(
        header::ETAG,
        etag_value.parse().expect("Invalid ETag value"),
    );
    parts.headers.insert(
        header::LAST_MODIFIED,
        format_http_date(now).parse().expect("Invalid Last-Modified value"),
    );
    parts.headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=3600"
            .parse()
            .expect("Invalid Cache-Control value"),
    );
    parts.headers.insert(
        header::VARY,
        "Accept-Encoding".parse().expect("Invalid Vary value"),
    );
    parts.headers.insert(
        "X-Cache-Status",
        "MISS".parse().expect("Invalid X-Cache-Status value"),
    );

    Response::from_parts(parts, Body::from(body_bytes))
}

/// Legacy middleware to add Cache-Control, ETag, and Vary headers to GET responses.
/// Handles conditional requests (If-None-Match) by returning 304 Not Modified.
/// This version does **not** store responses in the multi-layer cache.
pub async fn cache_control(req: Request<Body>, next: Next) -> Response {
    if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
        return next.run(req).await;
    }

    let if_none_match = req.headers().get(header::IF_NONE_MATCH).cloned();

    let response = next.run(req).await;

    if response.status() != StatusCode::OK {
        return response;
    }

    let (mut parts, body) = response.into_parts();

    let body_bytes = match to_bytes(body, 1_000_000).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to collect response body for caching: {}", e);
            return Response::from_parts(parts, Body::empty());
        }
    };

    let etag_value = generate_etag(&body_bytes);

    if let Some(inm) = if_none_match {
        if inm == etag_value.as_str() {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag_value)
                .header(header::CACHE_CONTROL, "public, max-age=3600")
                .header(header::VARY, "Accept-Encoding")
                .body(Body::empty())
                .expect("Failed to build 304 response");
        }
    }

    parts.headers.insert(
        header::ETAG,
        etag_value.parse().expect("Invalid ETag value"),
    );
    parts.headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=3600"
            .parse()
            .expect("Invalid Cache-Control value"),
    );
    parts.headers.insert(
        header::VARY,
        "Accept-Encoding".parse().expect("Invalid Vary value"),
    );

    Response::from_parts(parts, Body::from(body_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{middleware::from_fn, routing::get, Router};
    use axum_test::TestServer;

    async fn mock_handler() -> &'static str {
        "hello caching world"
    }

    async fn dynamic_handler() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn app() -> Router {
        Router::new()
            .route("/test", get(mock_handler))
            .route("/dynamic", get(dynamic_handler))
            .layer(from_fn(cache_control))
    }

    #[tokio::test]
    async fn adds_cache_headers() {
        let server = TestServer::new(app()).unwrap();
        let res = server.get("/test").await;

        res.assert_status_ok();
        res.assert_header(header::CACHE_CONTROL, "public, max-age=3600");
        res.assert_header(header::VARY, "Accept-Encoding");
        let etag = res.header(header::ETAG);
        assert!(etag.to_str().unwrap().starts_with("W/\""));
    }

    #[tokio::test]
    async fn returns_304_on_match() {
        let server = TestServer::new(app()).unwrap();

        // Initial request to get ETag
        let res1 = server.get("/test").await;
        let etag = res1.header(header::ETAG);

        // Conditional request
        let res2 = server
            .get("/test")
            .add_header(header::IF_NONE_MATCH, etag.clone())
            .await;

        assert_eq!(res2.status_code(), StatusCode::NOT_MODIFIED);
        assert_eq!(res2.header(header::ETAG), etag);
        assert!(res2.text().is_empty());
    }

    #[tokio::test]
    async fn etag_changes_on_update() {
        let server = TestServer::new(app()).unwrap();

        let res1 = server.get("/dynamic").await;
        let etag1 = res1.header(header::ETAG);

        let res2 = server.get("/dynamic").await;
        let etag2 = res2.header(header::ETAG);

        assert_ne!(
            etag1, etag2,
            "ETags should be different for different content"
        );
    }
}
