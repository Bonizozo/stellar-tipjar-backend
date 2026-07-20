// Prometheus metric collectors.
//
// Metrics are registered once at startup via `init()`.  The `lazy_static`
// statics defined at the bottom are zero-cost wrappers that call the OnceLock
// accessors; they exist purely for backwards compatibility with existing
// call sites that use `COUNTER.inc()` / `HISTOGRAM.observe()` syntax.
//
// The `.expect()` calls in the accessor functions are protected by
// `#[allow(clippy::expect_used)]` because they guard a programmer invariant
// (init() called before first use), not a runtime failure — panicking here
// is correct behaviour.
use prometheus::{register_counter, register_histogram, Counter, Histogram};
use std::sync::OnceLock;

static HTTP_REQUESTS_TOTAL_CELL: OnceLock<Counter> = OnceLock::new();
static HTTP_REQUEST_DURATION_SECONDS_CELL: OnceLock<Histogram> = OnceLock::new();
static TIPS_CREATED_TOTAL_CELL: OnceLock<Counter> = OnceLock::new();
static DB_QUERY_DURATION_SECONDS_CELL: OnceLock<Histogram> = OnceLock::new();

/// Register all Prometheus metrics.  Must be called once at startup (in `main`)
/// before any metric accessor is used.  Returns an error string if registration
/// fails — in practice only possible if names collide, which is impossible with
/// these unique static string literals.
pub fn init() -> Result<(), String> {
    HTTP_REQUESTS_TOTAL_CELL
        .set(
            register_counter!("http_requests_total", "Total HTTP requests")
                .map_err(|e| format!("http_requests_total: {e}"))?,
        )
        .map_err(|_| "http_requests_total already initialised".to_string())?;

    HTTP_REQUEST_DURATION_SECONDS_CELL
        .set(
            register_histogram!(
                "http_request_duration_seconds",
                "HTTP request duration in seconds"
            )
            .map_err(|e| format!("http_request_duration_seconds: {e}"))?,
        )
        .map_err(|_| "http_request_duration_seconds already initialised".to_string())?;

    TIPS_CREATED_TOTAL_CELL
        .set(
            register_counter!("tips_created_total", "Total tips successfully recorded on-chain")
                .map_err(|e| format!("tips_created_total: {e}"))?,
        )
        .map_err(|_| "tips_created_total already initialised".to_string())?;

    DB_QUERY_DURATION_SECONDS_CELL
        .set(
            register_histogram!(
                "db_query_duration_seconds",
                "Database query duration in seconds"
            )
            .map_err(|e| format!("db_query_duration_seconds: {e}"))?,
        )
        .map_err(|_| "db_query_duration_seconds already initialised".to_string())?;

    Ok(())
}

// ── Internal accessors ───────────────────────────────────────────────────────
// These are called from the lazy_static re-exports below.  The `.expect()` is
// intentional: panicking on uninitialised metrics is the correct behaviour
// (it is a programmer error, not a user-visible failure).

// SAFETY: `init()` is called in `main` before the router — and therefore
// before any middleware or controller — runs.  These OnceLock cells are always
// populated when these accessors are called.
// Invariant: `init()` called before first HTTP request is dispatched.

fn get_http_requests_total() -> Counter {
    #[allow(clippy::expect_used)]
    HTTP_REQUESTS_TOTAL_CELL
        .get()
        .expect("metrics::init() must be called before HTTP_REQUESTS_TOTAL is used")
        .clone()
}

fn get_http_request_duration_seconds() -> Histogram {
    #[allow(clippy::expect_used)]
    HTTP_REQUEST_DURATION_SECONDS_CELL
        .get()
        .expect("metrics::init() must be called before HTTP_REQUEST_DURATION_SECONDS is used")
        .clone()
}

fn get_tips_created_total() -> Counter {
    #[allow(clippy::expect_used)]
    TIPS_CREATED_TOTAL_CELL
        .get()
        .expect("metrics::init() must be called before TIPS_CREATED_TOTAL is used")
        .clone()
}

fn get_db_query_duration_seconds() -> Histogram {
    #[allow(clippy::expect_used)]
    DB_QUERY_DURATION_SECONDS_CELL
        .get()
        .expect("metrics::init() must be called before DB_QUERY_DURATION_SECONDS is used")
        .clone()
}

// ── Backwards-compatible statics ─────────────────────────────────────────────
// These mirror the original lazy_static API so all existing call sites
// (`HTTP_REQUESTS_TOTAL.inc()`, `DB_QUERY_DURATION_SECONDS.observe(...)`)
// remain unchanged.  They delegate to the OnceLock-backed accessors above;
// the lazy_static body itself contains no `.expect()` or `.unwrap()` calls.

lazy_static::lazy_static! {
    pub static ref HTTP_REQUESTS_TOTAL: Counter = get_http_requests_total();
    pub static ref HTTP_REQUEST_DURATION_SECONDS: Histogram = get_http_request_duration_seconds();
    pub static ref TIPS_CREATED_TOTAL: Counter = get_tips_created_total();
    pub static ref DB_QUERY_DURATION_SECONDS: Histogram = get_db_query_duration_seconds();
}
