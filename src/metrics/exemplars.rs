//! RED metrics (Rate / Errors / Duration) per route template with Prometheus
//! exemplar support.
//!
//! Exemplars link histogram observation buckets to the trace ID of the request
//! that landed in that bucket, so Grafana can jump directly from a slow bucket
//! in the latency heatmap to the corresponding Jaeger/Tempo trace.

use lazy_static::lazy_static;
use opentelemetry::trace::TraceContextExt as _;
use prometheus::{register_counter_vec, register_histogram_vec, CounterVec, HistogramVec};
use std::collections::HashMap;
use std::sync::Mutex;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

/// Histogram buckets for the request duration RED metric.
pub const HTTP_DURATION_BUCKETS: &[f64] =
    &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];

lazy_static! {
    /// Request rate per route template, method, and status class.
    pub static ref RED_REQUESTS_TOTAL: CounterVec = register_counter_vec!(
        "red_requests_total",
        "Total requests by route template, method, and status class",
        &["route", "method", "status_class"]
    )
    .unwrap();

    /// Error rate per route template and method.
    pub static ref RED_ERRORS_TOTAL: CounterVec = register_counter_vec!(
        "red_errors_total",
        "Total error responses (4xx/5xx) by route template and method",
        &["route", "method", "status"]
    )
    .unwrap();

    /// Request duration histogram per route template.
    pub static ref RED_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "red_request_duration_seconds",
        "Request duration in seconds with exemplar support, by route template and method",
        &["route", "method"],
        HTTP_DURATION_BUCKETS.to_vec()
    )
    .unwrap();

    /// Per-(route|method) exemplar store: most-recent sampled trace for each label set.
    static ref EXEMPLAR_STORE: Mutex<HashMap<String, Exemplar>> =
        Mutex::new(HashMap::new());
}

/// A single exemplar: the trace ID and the observed duration value.
#[derive(Clone, Default)]
pub struct Exemplar {
    pub trace_id: String,
    pub value: f64,
    pub timestamp_ms: u64,
}

/// Record one RED observation.
///
/// - `route`    — normalised route template (e.g. `/api/v1/tips/:id`)
/// - `method`   — HTTP method string
/// - `status`   — HTTP status code
/// - `duration` — elapsed time for this request
///
/// When the current span has a valid, sampled trace ID the observation is also
/// stored as an exemplar so the Prometheus text output can reference it.
pub fn record(route: &str, method: &str, status: u16, duration: std::time::Duration) {
    let status_class = match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    };

    RED_REQUESTS_TOTAL
        .with_label_values(&[route, method, status_class])
        .inc();

    if status >= 400 {
        RED_ERRORS_TOTAL
            .with_label_values(&[route, method, &status.to_string()])
            .inc();
    }

    let duration_secs = duration.as_secs_f64();
    RED_DURATION_SECONDS
        .with_label_values(&[route, method])
        .observe(duration_secs);

    // Attach exemplar — only when there is a valid, sampled OTel trace.
    let span = tracing::Span::current();
    let cx = span.context();
    let span_ref = cx.span();
    let sc = span_ref.span_context();
    if sc.is_sampled() && sc.is_valid() {
        let trace_id = sc.trace_id().to_string();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let key = format!("{}|{}", route, method);
        if let Ok(mut guard) = EXEMPLAR_STORE.lock() {
            guard.insert(
                key,
                Exemplar {
                    trace_id,
                    value: duration_secs,
                    timestamp_ms: ts,
                },
            );
        }
    }
}

/// Return a snapshot of all current exemplars keyed by `"route|method"`.
pub fn snapshot_exemplars() -> HashMap<String, Exemplar> {
    EXEMPLAR_STORE.lock().map(|g| g.clone()).unwrap_or_default()
}
