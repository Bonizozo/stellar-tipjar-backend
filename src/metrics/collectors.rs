use lazy_static::lazy_static;
use prometheus::{
    register_counter, register_counter_vec, register_gauge, register_gauge_vec,
    register_histogram, register_histogram_vec, Counter, CounterVec, Gauge, GaugeVec,
    Histogram, HistogramVec,
};

lazy_static! {
    // ── HTTP ──────────────────────────────────────────────────────────────────
    pub static ref HTTP_REQUESTS_TOTAL: CounterVec = register_counter_vec!(
        "http_requests_total",
        "Total HTTP requests by method, path, and status",
        &["method", "path", "status"]
    ).unwrap();

    pub static ref HTTP_REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "http_request_duration_seconds",
        "HTTP request duration in seconds by method and path",
        &["method", "path"],
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
    ).unwrap();

    pub static ref HTTP_REQUESTS_IN_FLIGHT: Gauge = register_gauge!(
        "http_requests_in_flight",
        "Number of HTTP requests currently being processed"
    ).unwrap();

    // ── Business: Tips ────────────────────────────────────────────────────────
    pub static ref TIPS_CREATED_TOTAL: Counter = register_counter!(
        "tips_created_total",
        "Total tips successfully recorded on-chain"
    ).unwrap();

    pub static ref TIPS_AMOUNT_XLM: Histogram = register_histogram!(
        "tips_amount_xlm",
        "Distribution of tip amounts in XLM",
        vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]
    ).unwrap();

    pub static ref TIPS_FAILED_TOTAL: CounterVec = register_counter_vec!(
        "tips_failed_total",
        "Total failed tip attempts by reason",
        &["reason"]
    ).unwrap();

    // ── Business: Creators ────────────────────────────────────────────────────
    pub static ref CREATORS_REGISTERED_TOTAL: Counter = register_counter!(
        "creators_registered_total",
        "Total creator accounts registered"
    ).unwrap();

    pub static ref CREATORS_ACTIVE: Gauge = register_gauge!(
        "creators_active",
        "Number of creators with at least one tip in the last 30 days"
    ).unwrap();

    // ── Blockchain Indexer ────────────────────────────────────────────────────
    pub static ref BLOCKCHAIN_EVENTS_PROCESSED_TOTAL: CounterVec = register_counter_vec!(
        "blockchain_events_processed_total",
        "Total blockchain events processed by type",
        &["event_type"]
    ).unwrap();

    pub static ref BLOCKCHAIN_EVENTS_FAILED_TOTAL: CounterVec = register_counter_vec!(
        "blockchain_events_failed_total",
        "Total blockchain event processing failures by reason",
        &["reason"]
    ).unwrap();

    pub static ref BLOCKCHAIN_INDEXER_LAG_LEDGERS: Gauge = register_gauge!(
        "blockchain_indexer_lag_ledgers",
        "Number of ledgers the indexer is behind the network tip"
    ).unwrap();

    pub static ref BLOCKCHAIN_RETRY_ATTEMPTS_TOTAL: Counter = register_counter!(
        "blockchain_retry_attempts_total",
        "Total SSE reconnect / retry attempts by the blockchain listener"
    ).unwrap();

    // ── Database ──────────────────────────────────────────────────────────────
    pub static ref DB_QUERY_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "db_query_duration_seconds",
        "Database query duration in seconds by operation",
        &["operation"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
    ).unwrap();

    pub static ref DB_POOL_CONNECTIONS: GaugeVec = register_gauge_vec!(
        "db_pool_connections",
        "Database connection pool size by state",
        &["state"]   // "idle" | "active"
    ).unwrap();

    // ── Cache ─────────────────────────────────────────────────────────────────
    pub static ref CACHE_HITS_TOTAL: CounterVec = register_counter_vec!(
        "cache_hits_total",
        "Cache hits by layer",
        &["layer"]
    ).unwrap();

    pub static ref CACHE_MISSES_TOTAL: CounterVec = register_counter_vec!(
        "cache_misses_total",
        "Cache misses by layer",
        &["layer"]
    ).unwrap();

    // ── Auth ──────────────────────────────────────────────────────────────────
    pub static ref AUTH_FAILURES_TOTAL: CounterVec = register_counter_vec!(
        "auth_failures_total",
        "Authentication failures by reason",
        &["reason"]
    ).unwrap();
}

/// Aggregated snapshot used by the `/metrics/summary` endpoint.
#[derive(serde::Serialize)]
pub struct MetricsSummary {
    pub tips_created_total: f64,
    pub creators_active: f64,
    pub blockchain_indexer_lag_ledgers: f64,
    pub http_requests_in_flight: f64,
}

pub fn collect_summary() -> MetricsSummary {
    MetricsSummary {
        tips_created_total: TIPS_CREATED_TOTAL.get(),
        creators_active: CREATORS_ACTIVE.get(),
        blockchain_indexer_lag_ledgers: BLOCKCHAIN_INDEXER_LAG_LEDGERS.get(),
        http_requests_in_flight: HTTP_REQUESTS_IN_FLIGHT.get(),
    }
}
