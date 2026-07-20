pub mod collectors;

pub use collectors::init as init_metrics;

use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
};
use prometheus::{Encoder, TextEncoder};

pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];

    let metric_families = prometheus::gather();
    // SAFETY: TextEncoder::encode writes to a Vec<u8>, which is infallible.
    // The only error path is an I/O error on the underlying writer; Vec never
    // returns an I/O error.  Invariant: encode into Vec<u8> always succeeds.
    let _ = encoder.encode(&metric_families, &mut buffer);

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        buffer,
    )
}
