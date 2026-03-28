use axum::http::StatusCode;
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;

/// Default request timeout (30 seconds).
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Returns a [`TimeoutLayer`] with the given duration.
/// On timeout, axum will return a `408 Request Timeout` response automatically.
pub fn timeout_layer(duration: Duration) -> TimeoutLayer {
    TimeoutLayer::new(duration)
}

/// Returns a [`TimeoutLayer`] using the `REQUEST_TIMEOUT_SECS` env var,
/// falling back to [`DEFAULT_TIMEOUT_SECS`].
pub fn timeout_layer_from_env() -> TimeoutLayer {
    let secs = std::env::var("REQUEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    timeout_layer(Duration::from_secs(secs))
}

/// Maps a timeout error body to a `408 Request Timeout` response.
/// Wire this up as a `handle_error` layer if you need a custom body.
pub fn on_timeout() -> (StatusCode, &'static str) {
    (StatusCode::REQUEST_TIMEOUT, "Request timed out")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use axum_test::TestServer;
    use std::time::Duration;
    use tower::ServiceBuilder;
    use tower_http::timeout::TimeoutLayer;

    async fn fast_handler() -> &'static str {
        "ok"
    }

    async fn slow_handler() -> &'static str {
        tokio::time::sleep(Duration::from_secs(10)).await;
        "too late"
    }

    fn app(timeout: Duration) -> Router {
        Router::new()
            .route("/fast", get(fast_handler))
            .route("/slow", get(slow_handler))
            .layer(ServiceBuilder::new().layer(TimeoutLayer::new(timeout)))
    }

    #[tokio::test]
    async fn fast_request_succeeds() {
        let server = TestServer::new(app(Duration::from_secs(5))).unwrap();
        let res = server.get("/fast").await;
        res.assert_status_ok();
        res.assert_text("ok");
    }

    #[tokio::test]
    async fn slow_request_returns_408() {
        let server = TestServer::new(app(Duration::from_millis(50))).unwrap();
        let res = server.get("/slow").await;
        assert_eq!(res.status_code(), 408);
    }

    #[tokio::test]
    async fn timeout_layer_from_env_uses_default() {
        std::env::remove_var("REQUEST_TIMEOUT_SECS");
        // Just verify it constructs without panic
        let _layer = timeout_layer_from_env();
    }

    #[tokio::test]
    async fn timeout_layer_from_env_reads_env_var() {
        std::env::set_var("REQUEST_TIMEOUT_SECS", "60");
        let _layer = timeout_layer_from_env();
        std::env::remove_var("REQUEST_TIMEOUT_SECS");
    }
}
