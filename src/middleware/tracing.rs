use axum::{extract::Request, middleware::Next, response::Response};
use opentelemetry_semantic_conventions::trace::{HTTP_REQUEST_METHOD, HTTP_ROUTE, HTTP_RESPONSE_STATUS_CODE};
use tracing::Instrument;

/// Axum middleware that opens a root OTel-aware span for every HTTP request,
/// recording method, route, and response status as span attributes.
pub async fn trace_request(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();

    // Extract parent context from W3C traceparent/tracestate headers.
    let parent_cx = crate::telemetry::extract_context(req.headers());
    let _guard = opentelemetry::Context::attach(parent_cx);

    let span = tracing::info_span!(
        "http_request",
        { HTTP_REQUEST_METHOD } = %method,
        { HTTP_ROUTE } = %path,
        { HTTP_RESPONSE_STATUS_CODE } = tracing::field::Empty,
    );

    let response = next.run(req).instrument(span.clone()).await;

    span.record(HTTP_RESPONSE_STATUS_CODE, response.status().as_u16());
    response
}
