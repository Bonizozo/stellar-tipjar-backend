use axum::{
    body::{Body, to_bytes},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use sha2::{Sha256, Digest};
use base64::{engine::general_purpose, Engine as _};

/// Middleware to add Cache-Control, ETag, and Vary headers to GET responses.
/// Handles conditional requests (If-None-Match) by returning 304 Not Modified.
pub async fn cache_control(req: Request<Body>, next: Next) -> Response {
    // Only apply caching to GET requests
    if req.method() != axum::http::Method::GET {
        return next.run(req).await;
    }

    let if_none_match = req.headers().get(header::IF_NONE_MATCH).cloned();
    
    let response = next.run(req).await;
    
    // Only cache successful 200 OK responses
    if response.status() != StatusCode::OK {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    
    // Collect body into bytes to calculate ETag (limit to 1MB)
    let body_bytes = match to_bytes(body, 1_000_000).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to collect response body for caching: {}", e);
            // Return empty response on error if we had already consumed parts
            return Response::from_parts(parts, Body::empty());
        }
    };

    // Generate ETag (strong or weak)
    let mut hasher = Sha256::new();
    hasher.update(&body_bytes);
    let hash = hasher.finalize();
    let etag_value = format!("W/\"{}\"", general_purpose::STANDARD.encode(hash));

    // Check If-None-Match condition
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

    // Insert headers
    parts.headers.insert(header::ETAG, etag_value.parse().expect("Invalid ETag value"));
    parts.headers.insert(header::CACHE_CONTROL, "public, max-age=3600".parse().expect("Invalid Cache-Control value"));
    parts.headers.insert(header::VARY, "Accept-Encoding".parse().expect("Invalid Vary value"));

    Response::from_parts(parts, Body::from(body_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get, middleware::from_fn};
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
        let res2 = server.get("/test")
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

        assert_ne!(etag1, etag2, "ETags should be different for different content");
    }
}
