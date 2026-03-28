use crate::metrics::collectors::{HTTP_REQUESTS_TOTAL, HTTP_REQUEST_DURATION_SECONDS};
use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

pub async fn track_metrics(req: Request, next: Next) -> Response {
    let start = Instant::now();

    // Increment the total request counter immediately
    HTTP_REQUESTS_TOTAL.inc();

    // Process the request
    let response = next.run(req).await;

    // Calculate duration and record it in the histogram
    let duration = start.elapsed();
    HTTP_REQUEST_DURATION_SECONDS.observe(duration.as_secs_f64());

    response
}
