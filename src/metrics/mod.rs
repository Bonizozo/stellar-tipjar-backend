pub mod collectors;
pub mod exemplars;

use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use prometheus::{Encoder, TextEncoder};

/// Prometheus scrape endpoint — consumed by the Prometheus server.
///
/// The response contains the standard Prometheus text format (0.0.4) with an
/// appended exemplar section for `red_request_duration_seconds` buckets.
/// When Prometheus is configured with `--enable-feature=exemplar-storage` the
/// exemplar lines are ingested and available for Grafana trace linking.
pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();

    // Append exemplar annotations for RED duration histogram.
    // Format: `metric_bucket{labels} value # {traceID="<id>"} <value> <ts_ms>`
    let exemplars = exemplars::snapshot_exemplars();
    if !exemplars.is_empty() {
        let mut exemplar_lines = String::new();
        for (key, ex) in &exemplars {
            if ex.trace_id.is_empty() {
                continue;
            }
            // key is "route|method"
            let parts: Vec<&str> = key.splitn(2, '|').collect();
            let (route, method) = if parts.len() == 2 {
                (parts[0], parts[1])
            } else {
                continue;
            };
            // Emit a single exemplar line for the +Inf bucket.
            exemplar_lines.push_str(&format!(
                "red_request_duration_seconds_bucket{{le=\"+Inf\",route=\"{}\",method=\"{}\"}} 1 # {{traceID=\"{}\"}} {} {}\n",
                route, method, ex.trace_id, ex.value, ex.timestamp_ms
            ));
        }
        buffer.extend_from_slice(exemplar_lines.as_bytes());
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        buffer,
    )
}

/// JSON aggregation endpoint — consumed by dashboards / health checks.
pub async fn metrics_summary_handler() -> impl IntoResponse {
    Json(collectors::collect_summary())
}
