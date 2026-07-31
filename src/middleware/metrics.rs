use crate::metrics::collectors::{
    HTTP_REQUESTS_IN_FLIGHT, HTTP_REQUESTS_TOTAL, HTTP_REQUEST_DURATION_SECONDS,
};
use crate::metrics::exemplars;
use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

pub async fn track_metrics(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    HTTP_REQUESTS_IN_FLIGHT.inc();
    let start = Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed();
    let duration_secs = duration.as_secs_f64();
    let status = response.status().as_u16();
    let status_str = status.to_string();

    // Legacy Prometheus counters (kept for backwards compat with existing dashboards).
    HTTP_REQUESTS_TOTAL
        .with_label_values(&[&method, &path, &status_str])
        .inc();
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[&method, &path])
        .observe(duration_secs);
    HTTP_REQUESTS_IN_FLIGHT.dec();

    // RED metrics with exemplar support (trace_id linked to slow buckets).
    exemplars::record(&path, &method, status, duration);

    response
}
