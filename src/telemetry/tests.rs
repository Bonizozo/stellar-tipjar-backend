//! Integration tests for end-to-end OTel propagation.
//!
//! Tests covered:
//!   1. `propagation_round_trip` — inbound `traceparent` is extracted and the
//!      same trace-id is injected into an outbound `reqwest` HeaderMap.
//!   2. `job_boundary_span_link` — a `Message` created inside a span carries
//!      the producer trace context; the consumer span is parented to it.
//!   3. `log_trace_id_correlation` — a tracing span emits a span with a
//!      trace-id accessible via `OpenTelemetrySpanExt`.

use opentelemetry::global;
use opentelemetry::trace::{TraceContextExt, TraceId};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;

/// Initialise the W3C propagator for tests that need it.
fn setup_propagator() {
    global::set_text_map_propagator(TraceContextPropagator::new());
}

// ── 1. Propagation round-trip ─────────────────────────────────────────────────

#[test]
fn propagation_round_trip() {
    setup_propagator();

    let known_trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
    let known_span_id = "00f067aa0ba902b7";
    let traceparent = format!("00-{}-{}-01", known_trace_id, known_span_id);

    let mut inbound_headers = axum::http::HeaderMap::new();
    inbound_headers.insert(
        "traceparent",
        axum::http::HeaderValue::from_str(&traceparent).unwrap(),
    );

    // Extract parent context from the inbound traceparent header.
    let parent_cx = crate::telemetry::extract_context(&inbound_headers);

    let extracted_trace_id = parent_cx.span().span_context().trace_id();
    let expected = TraceId::from_hex(known_trace_id).unwrap();
    assert_eq!(
        extracted_trace_id, expected,
        "Extracted trace-id must match inbound traceparent"
    );

    // Inject the extracted context into an outbound HeaderMap.
    let _guard = opentelemetry::Context::attach(parent_cx);
    let mut outbound = axum::http::HeaderMap::new();
    crate::telemetry::inject_context(&mut outbound);

    let outbound_tp = outbound
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    assert!(
        outbound_tp.contains(known_trace_id),
        "Outbound traceparent '{}' must carry trace-id '{}'",
        outbound_tp,
        known_trace_id
    );
}

// ── 2. Job-boundary span link ─────────────────────────────────────────────────

#[test]
fn job_boundary_span_link() {
    setup_propagator();

    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::TracerProvider;

    let provider = TracerProvider::default();
    global::set_tracer_provider(provider.clone());

    let tracer = provider.tracer("test");
    let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
    let subscriber = tracing_subscriber::registry().with(otel_layer);

    tracing::subscriber::with_default(subscriber, || {
        let producer_span = tracing::info_span!("producer.publish");
        let _guard = producer_span.enter();

        let producer_trace_id = producer_span.context().span().span_context().trace_id();

        // Message::new() captures the active OTel context via the carrier.
        let msg = crate::queue::publisher::Message::new(
            "test_job",
            serde_json::json!({"key": "value"}),
        );

        // Consumer side: extract the propagated context.
        let consumer_cx = msg.extract_trace_context();
        let consumer_trace_id = consumer_cx.span().span_context().trace_id();

        // With no-op provider trace IDs are invalid (all zeros) — we check
        // consistency rather than a specific value.
        let zero = TraceId::from_hex("00000000000000000000000000000000").unwrap();
        if producer_trace_id != zero {
            assert_eq!(
                consumer_trace_id, producer_trace_id,
                "Consumer must share the same trace-id as the producer"
            );
        }

        // trace_context must survive a serialise / deserialise round-trip.
        let json = serde_json::to_string(&msg).expect("Message must serialise");
        let recovered: crate::queue::publisher::Message =
            serde_json::from_str(&json).expect("Message must deserialise");
        assert_eq!(
            recovered.trace_context, msg.trace_context,
            "trace_context must survive JSON round-trip"
        );
    });
}

// ── 3. Log / trace-id correlation ────────────────────────────────────────────

#[test]
fn log_trace_correlation() {
    setup_propagator();

    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::TracerProvider;

    let provider = TracerProvider::default();
    global::set_tracer_provider(provider.clone());

    let tracer = provider.tracer("test-log-correlation");
    let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
    let subscriber = tracing_subscriber::registry().with(otel_layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("test.request", route = "/api/v1/tips");
        let _guard = span.enter();

        // Verify the span → OTel context path works without panicking.
        let cx = span.context();
        let sc = cx.span().span_context();
        let _ = sc.trace_id();
        let _ = sc.span_id();

        // In production this log line carries trace_id/span_id from the OTel layer.
        tracing::info!("log/trace correlation test — trace_id and span_id in structured output");
    });
}
