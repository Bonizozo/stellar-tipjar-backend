use axum::{extract::Request, middleware::Next, response::Response};
use opentelemetry::trace::TraceContextExt as _;
use opentelemetry_semantic_conventions::trace::{
    HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, HTTP_ROUTE,
};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

/// Axum middleware that opens a root OTel-aware span for every HTTP request.
///
/// For each request it:
/// 1. Extracts a parent context from incoming W3C `traceparent` / `tracestate`
///    headers so distributed traces are correctly linked across services.
/// 2. Creates a `http_request` span with standard semantic-convention attributes
///    (`http.request.method`, `http.route`, `http.response.status_code`).
/// 3. Injects the active `trace-id` into the `x-trace-id` response header so
///    callers can correlate their requests with backend traces.
pub async fn trace_request(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();

    // Extract parent context from W3C traceparent/tracestate headers.
    let parent_cx = crate::telemetry::extract_context(req.headers());
    let _guard = opentelemetry::Context::attach(parent_cx.clone());

    let span = tracing::info_span!(
        "http_request",
        { HTTP_REQUEST_METHOD } = %method,
        { HTTP_ROUTE } = %path,
        { HTTP_RESPONSE_STATUS_CODE } = tracing::field::Empty,
    );

    // Link the tracing span to the extracted OTel parent context so the span
    // is correctly parented in the distributed trace.
    span.set_parent(parent_cx);

    // Capture the trace-id before we move into the instrumented future.
    let trace_id = {
        let cx = span.context();
        let span_ref = cx.span();
        let sc = span_ref.span_context();
        if sc.is_valid() {
            Some(sc.trace_id().to_string())
        } else {
            None
        }
    };

    let mut response = next.run(req).instrument(span.clone()).await;

    span.record(HTTP_RESPONSE_STATUS_CODE, response.status().as_u16());

    // Propagate the trace-id to the caller via a response header so they can
    // include it in bug reports / support tickets.
    if let Some(tid) = trace_id {
        if let Ok(value) = axum::http::HeaderValue::from_str(&tid) {
            response
                .headers_mut()
                .insert("x-trace-id", value);
        }
    }

    response
}
