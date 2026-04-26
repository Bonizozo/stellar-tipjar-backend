use crate::metrics::collectors::{
    HTTP_REQUEST_DURATION_SECONDS, HTTP_REQUESTS_IN_FLIGHT, HTTP_REQUESTS_TOTAL,
};
use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

pub async fn track_metrics(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    HTTP_REQUESTS_IN_FLIGHT.inc();
    let start = Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    HTTP_REQUESTS_TOTAL
        .with_label_values(&[&method, &path, &status])
        .inc();
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[&method, &path])
        .observe(duration);
    HTTP_REQUESTS_IN_FLIGHT.dec();

    response
}
