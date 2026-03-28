use lazy_static::lazy_static;
use prometheus::{register_counter, register_histogram, Counter, Histogram};

lazy_static! {
    pub static ref HTTP_REQUESTS_TOTAL: Counter = register_counter!(
        "http_requests_total",
        "Total HTTP requests"
    ).unwrap();

    pub static ref HTTP_REQUEST_DURATION_SECONDS: Histogram = register_histogram!(
        "http_request_duration_seconds",
        "HTTP request duration in seconds"
    ).unwrap();

    pub static ref TIPS_CREATED_TOTAL: Counter = register_counter!(
        "tips_created_total",
        "Total tips successfully recorded on-chain"
    ).unwrap();

    // Custom business metric: track database query duration
    pub static ref DB_QUERY_DURATION_SECONDS: Histogram = register_histogram!(
        "db_query_duration_seconds",
        "Database query duration in seconds"
    ).unwrap();
}
