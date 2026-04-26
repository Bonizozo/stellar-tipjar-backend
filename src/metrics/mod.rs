pub mod collectors;

use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use prometheus::{Encoder, TextEncoder};

/// Prometheus scrape endpoint — consumed by the Prometheus server.
pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
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
