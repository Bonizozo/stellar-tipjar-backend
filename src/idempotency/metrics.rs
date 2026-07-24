use lazy_static::lazy_static;
use prometheus::{register_counter, register_histogram, Counter, Histogram};

lazy_static! {
    pub static ref IDEMPOTENCY_REPLAY_TOTAL: Counter = register_counter!(
        "idempotency_replay_total",
        "Requests served from a cached idempotent response instead of re-executing"
    )
    .unwrap();

    pub static ref IDEMPOTENCY_CONFLICT_TOTAL: Counter = register_counter!(
        "idempotency_conflict_total",
        "Requests rejected with 409 because a duplicate with the same key was already in flight"
    )
    .unwrap();

    pub static ref IDEMPOTENCY_MISMATCH_TOTAL: Counter = register_counter!(
        "idempotency_mismatch_total",
        "Requests rejected with 422 because the Idempotency-Key was reused with a different request body"
    )
    .unwrap();

    pub static ref IDEMPOTENCY_EXECUTED_TOTAL: Counter = register_counter!(
        "idempotency_executed_total",
        "Requests that acquired the idempotency lock and executed the handler"
    )
    .unwrap();

    pub static ref IDEMPOTENCY_REDIS_FALLBACK_TOTAL: Counter = register_counter!(
        "idempotency_redis_fallback_total",
        "Requests that fell back to the Postgres idempotency store because Redis was unavailable"
    )
    .unwrap();

    pub static ref IDEMPOTENCY_BODY_TOO_LARGE_TOTAL: Counter = register_counter!(
        "idempotency_body_too_large_total",
        "Completed responses whose body exceeded the idempotency cache cap and were not persisted"
    )
    .unwrap();

    /// Time spent in the begin() lock-acquire/conflict-detection step.
    /// A rising p99 here is the leading indicator of lock contention on hot
    /// idempotency keys (or of Postgres-fallback latency when Redis is down).
    pub static ref IDEMPOTENCY_LOCK_CONTENTION_SECONDS: Histogram = register_histogram!(
        "idempotency_lock_contention_seconds",
        "Time spent acquiring (or failing to acquire) the idempotency execution lock",
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5]
    )
    .unwrap();
}
