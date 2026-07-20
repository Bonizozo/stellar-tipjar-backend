use axum::http::StatusCode;
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;

/// Default request timeout (30 seconds).
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Returns a [`TimeoutLayer`] with the given duration.
///
/// The duration comes from `AppConfig::request_timeout`, which is validated
/// once at startup — no `std::env::var` reads happen here.
pub fn timeout_layer(duration: Duration) -> TimeoutLayer {
    TimeoutLayer::new(duration)
}

/// Maps a timeout error body to a `408 Request Timeout` response.
pub fn on_timeout() -> (StatusCode, &'static str) {
    (StatusCode::REQUEST_TIMEOUT, "Request timed out")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use axum_test::TestServer;
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
}
