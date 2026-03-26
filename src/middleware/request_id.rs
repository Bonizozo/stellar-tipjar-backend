use axum::{extract::Request, middleware::Next, response::Response};
use tower_http::request_id::RequestId;
use tracing::Instrument;

/// Extracts the `x-request-id` header injected by `tower_http::SetRequestIdLayer`
/// and attaches it as a tracing span field so every log line within the request
/// carries the same `request_id`.
pub async fn propagate_request_id(req: Request, next: Next) -> Response {
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("unknown")
        .to_owned();

    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
    );

    next.run(req).instrument(span).await
}
