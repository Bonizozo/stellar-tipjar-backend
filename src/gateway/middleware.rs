use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::Request,
    http::HeaderValue,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::errors::AppError;
use crate::gateway::context::GatewayIdentity;

// ── API-key scope enforcement ─────────────────────────────────────────────────

/// Axum middleware factory: require the caller to hold a specific permission.
///
/// Must run **after** `gateway_auth` (which injects `GatewayIdentity`).
///
/// JWT callers always pass (role-based checks happen in `authorization`
/// middleware downstream).  API-key callers are checked against their
/// explicit permission list.  Anonymous callers are always rejected.
///
/// # Example
/// ```rust
/// .layer(axum::middleware::from_fn(|req, next| {
///     require_scope("tips:write", req, next)
/// }))
/// ```
pub async fn require_scope(
    scope: &'static str,
    req: Request,
    next: Next,
) -> Response {
    let identity = req.extensions().get::<GatewayIdentity>().cloned();

    match identity {
        Some(GatewayIdentity::Jwt { .. }) => next.run(req).await,
        Some(GatewayIdentity::ApiKey { ref permissions, .. }) => {
            if permissions.iter().any(|p| p == scope || p == "*") {
                next.run(req).await
            } else {
                AppError::forbidden(format!(
                    "API key does not have the '{}' permission",
                    scope
                ))
                .into_response()
            }
        }
        Some(GatewayIdentity::Anonymous) | None => {
            AppError::unauthorized("Authentication required").into_response()
        }
    }
}

// ── Gateway metrics middleware ────────────────────────────────────────────────

/// Axum middleware that records gateway-level metrics on every request:
///
/// - `x-gateway-latency-ms` response header (total gateway processing time).
/// - Structured log line with method, path, status, latency, and caller identity.
///
/// This runs at the outermost layer so it captures the full round-trip time
/// including all inner middleware.
pub async fn gateway_metrics(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();
    let identity = req
        .extensions()
        .get::<GatewayIdentity>()
        .map(|id| id.display())
        .unwrap_or_else(|| "unknown".to_string());

    let start = Instant::now();
    let mut response = next.run(req).await;
    let latency_ms = start.elapsed().as_millis();

    let status = response.status().as_u16();

    tracing::info!(
        method   = %method,
        path     = %path,
        status   = status,
        latency_ms = latency_ms,
        identity = %identity,
        "gateway request"
    );

    // Inject latency header for client-side diagnostics.
    if let Ok(v) = HeaderValue::from_str(&latency_ms.to_string()) {
        response.headers_mut().insert("x-gateway-latency-ms", v);
    }

    response
}

// ── Request ID propagation ────────────────────────────────────────────────────

/// Inject the `x-request-id` value (set by `tower_http::SetRequestIdLayer`)
/// into the response so callers can correlate requests with server logs.
pub async fn propagate_request_id_to_response(req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .cloned();

    let mut response = next.run(req).await;

    if let Some(id) = request_id {
        response.headers_mut().insert("x-request-id", id);
    }

    response
}

// ── Caller identity header ────────────────────────────────────────────────────

/// Inject `X-Authenticated-As` into the response for debugging.
/// Only injected in non-production environments.
pub async fn inject_identity_header(req: Request, next: Next) -> Response {
    let is_dev = std::env::var("DEPLOYMENT_ENVIRONMENT")
        .map(|e| e == "development" || e == "staging")
        .unwrap_or(true);

    let identity_str = req
        .extensions()
        .get::<GatewayIdentity>()
        .map(|id| id.display());

    let mut response = next.run(req).await;

    if is_dev {
        if let Some(id) = identity_str {
            if let Ok(v) = HeaderValue::from_str(&id) {
                response.headers_mut().insert("x-authenticated-as", v);
            }
        }
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{middleware::from_fn, routing::get, Router};
    use axum_test::TestServer;

    async fn ok_handler() -> &'static str {
        "ok"
    }

    #[tokio::test]
    async fn gateway_metrics_adds_latency_header() {
        let app = Router::new()
            .route("/test", get(ok_handler))
            .layer(from_fn(gateway_metrics));

        let server = TestServer::new(app).unwrap();
        let res = server.get("/test").await;
        res.assert_status_ok();
        assert!(
            res.headers().get("x-gateway-latency-ms").is_some(),
            "x-gateway-latency-ms header should be present"
        );
    }

    #[tokio::test]
    async fn propagate_request_id_to_response_works() {
        let app = Router::new()
            .route("/test", get(ok_handler))
            .layer(from_fn(propagate_request_id_to_response));

        let server = TestServer::new(app).unwrap();
        let res = server
            .get("/test")
            .add_header(
                axum::http::HeaderName::from_static("x-request-id"),
                axum::http::HeaderValue::from_static("test-id-123"),
            )
            .await;
        res.assert_status_ok();
        assert_eq!(
            res.headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok()),
            Some("test-id-123")
        );
    }
}
