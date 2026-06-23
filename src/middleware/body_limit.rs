use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tower_http::limit::RequestBodyLimitLayer;

/// Default maximum request body size: 1 MiB.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Returns a [`RequestBodyLimitLayer`] sized from the `MAX_REQUEST_BODY_BYTES`
/// environment variable, falling back to [`DEFAULT_BODY_LIMIT_BYTES`] (1 MiB).
///
/// Oversized bodies cause axum's extractors (`Json`, `Bytes`, `String`) to
/// return **413 Payload Too Large** automatically.
pub fn body_limit_layer_from_env() -> RequestBodyLimitLayer {
    let limit = std::env::var("MAX_REQUEST_BODY_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_BODY_LIMIT_BYTES);
    tracing::debug!(limit_bytes = limit, "body size limit configured");
    RequestBodyLimitLayer::new(limit)
}

/// Middleware that rejects write requests whose `Content-Type` is not
/// `application/json` with **415 Unsupported Media Type**.
///
/// Only POST, PUT, and PATCH requests are inspected; all other methods pass
/// through. Apply this to API routers that exclusively consume JSON.
pub async fn require_json_content_type(req: Request<Body>, next: Next) -> Response {
    if matches!(*req.method(), Method::POST | Method::PUT | Method::PATCH) {
        let content_type = req
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.contains("application/json") {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                axum::Json(serde_json::json!({
                    "error": "Unsupported Media Type",
                    "code": "UNSUPPORTED_MEDIA_TYPE",
                    "status": 415,
                    "details": { "required": "application/json" }
                })),
            )
                .into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware, routing::post, Json, Router};
    use serde_json::json;
    use tower::ServiceExt;

    async fn json_echo(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
        Json(body)
    }

    fn limited_app(limit: usize) -> Router {
        Router::new()
            .route("/echo", post(json_echo))
            .layer(tower_http::limit::RequestBodyLimitLayer::new(limit))
    }

    fn content_type_app() -> Router {
        Router::new()
            .route("/data", post(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(require_json_content_type))
    }

    async fn post_request(app: Router, uri: &str, content_type: &str, body: impl Into<Body>) -> StatusCode {
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", content_type)
                .body(body.into())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }

    #[tokio::test]
    async fn normal_json_body_passes() {
        let app = limited_app(1024 * 1024);
        let payload = serde_json::to_vec(&json!({"hello": "world"})).unwrap();
        let status = post_request(app, "/echo", "application/json", payload).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn oversized_body_returns_413() {
        // 64-byte hard limit; the JSON payload will exceed it.
        let app = limited_app(64);
        let payload = serde_json::to_vec(&json!({"data": "a".repeat(200)})).unwrap();
        assert!(payload.len() > 64, "payload must exceed the configured limit");
        let status = post_request(app, "/echo", "application/json", payload).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn non_json_content_type_returns_415() {
        let app = content_type_app();
        let status = post_request(app, "/data", "text/plain", Body::from("hello")).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn missing_content_type_returns_415() {
        let app = content_type_app();
        // No content-type header → empty string → not application/json → 415
        let status = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/data")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn json_content_type_with_charset_passes() {
        let app = content_type_app();
        let status = post_request(app, "/data", "application/json; charset=utf-8", Body::from("{}")).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn get_request_bypasses_content_type_check() {
        let app = Router::new()
            .route("/data", axum::routing::get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(require_json_content_type));
        let status = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/data")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn body_limit_from_env_uses_default() {
        std::env::remove_var("MAX_REQUEST_BODY_BYTES");
        let _layer = body_limit_layer_from_env();
    }

    #[tokio::test]
    async fn body_limit_from_env_custom_value() {
        std::env::set_var("MAX_REQUEST_BODY_BYTES", "2097152");
        let _layer = body_limit_layer_from_env();
        std::env::remove_var("MAX_REQUEST_BODY_BYTES");
    }

    #[tokio::test]
    async fn body_limit_from_env_ignores_invalid_value() {
        std::env::set_var("MAX_REQUEST_BODY_BYTES", "not-a-number");
        let _layer = body_limit_layer_from_env(); // must not panic; falls back to default
        std::env::remove_var("MAX_REQUEST_BODY_BYTES");
    }
}
