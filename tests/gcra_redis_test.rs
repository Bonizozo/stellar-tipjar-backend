//! Integration tests for the Redis-backed GCRA limiter
//! (`gateway::rate_limit_script::GcraLimiter`) against a *real* Redis
//! instance — these exercise the actual Lua script, not the pure-Rust
//! mirror used for the fast unit tests in `rate_limit_script.rs`.
//!
//! Requires `TEST_REDIS_URL` (or `REDIS_URL`) to point at a reachable Redis.
//! Skips gracefully (prints a message, returns early) when neither is set or
//! the connection fails, matching the existing pattern for DB-dependent
//! tests in this suite.

use redis::aio::ConnectionManager;
use stellar_tipjar_backend::gateway::rate_limit_script::GcraLimiter;

async fn connect() -> Option<ConnectionManager> {
    let url = std::env::var("TEST_REDIS_URL")
        .or_else(|_| std::env::var("REDIS_URL"))
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let client = redis::Client::open(url.as_str()).ok()?;
    match ConnectionManager::new(client).await {
        Ok(conn) => Some(conn),
        Err(e) => {
            eprintln!("skipping gcra_redis_test: cannot reach Redis at {url}: {e}");
            None
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn unique_key(prefix: &str) -> String {
    // Avoid cross-test collisions without needing a real clock/random source
    // dependency; PID + a static counter is enough uniqueness for a single
    // test binary run.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("test:gcra:{}:{}:{}", prefix, std::process::id(), n)
}

#[tokio::test]
async fn admits_exactly_burst_then_rejects() {
    let Some(mut conn) = connect().await else {
        return;
    };
    let limiter = GcraLimiter::new();
    let key = unique_key("burst");
    let now = now_ms();

    // limit=600/min (10 req/s) with burst=5: the first 5 immediate requests
    // should be admitted, the 6th should not.
    let mut admitted = 0;
    for _ in 0..5 {
        let d = limiter
            .check(&mut conn, &key, 600, 5, 60, now)
            .await
            .unwrap();
        if d.allowed {
            admitted += 1;
        }
    }
    assert_eq!(admitted, 5, "all 5 burst slots should be admitted");

    let sixth = limiter
        .check(&mut conn, &key, 600, 5, 60, now)
        .await
        .unwrap();
    assert!(!sixth.allowed, "6th immediate request must be rejected");
    assert!(sixth.retry_after_secs > 0);
}

#[tokio::test]
async fn concurrent_hammering_admits_no_more_than_burst() {
    // The core "no read-modify-write race" guarantee: fire many more
    // concurrent requests than the burst allowance at the *same* key and
    // verify the admitted count never exceeds the configured burst, even
    // under real concurrency against real Redis.
    let Some(conn) = connect().await else {
        return;
    };
    let limiter = std::sync::Arc::new(GcraLimiter::new());
    let key = std::sync::Arc::new(unique_key("concurrent"));
    let now = now_ms();
    let burst: u64 = 10;
    let concurrency = 50;

    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let mut conn = conn.clone();
        let limiter = std::sync::Arc::clone(&limiter);
        let key = std::sync::Arc::clone(&key);
        handles.push(tokio::spawn(async move {
            limiter
                .check(&mut conn, &key, 60, burst, 60, now)
                .await
                .unwrap()
                .allowed
        }));
    }

    let mut admitted = 0u64;
    for h in handles {
        if h.await.unwrap() {
            admitted += 1;
        }
    }

    assert_eq!(
        admitted, burst,
        "exactly {burst} of {concurrency} concurrent requests should be admitted (got {admitted}) — \
         any other number indicates a read-modify-write race"
    );
}

#[tokio::test]
async fn window_rollover_recovers_capacity() {
    let Some(mut conn) = connect().await else {
        return;
    };
    let limiter = GcraLimiter::new();
    let key = unique_key("rollover");
    let now = now_ms();

    // limit=60/min (1 req/s), burst=3: consume the full burst immediately.
    for _ in 0..3 {
        let d = limiter
            .check(&mut conn, &key, 60, 3, 60, now)
            .await
            .unwrap();
        assert!(d.allowed);
    }
    let exhausted = limiter
        .check(&mut conn, &key, 60, 3, 60, now)
        .await
        .unwrap();
    assert!(!exhausted.allowed);

    // Advancing the clock a full burst window (3 * 1000ms = 3000ms) should
    // fully recover capacity.
    let recovered_now = now + 3100;
    for i in 0..3 {
        let d = limiter
            .check(&mut conn, &key, 60, 3, 60, recovered_now)
            .await
            .unwrap();
        assert!(d.allowed, "recovered request {i} should be admitted");
    }
}

#[tokio::test]
async fn rejected_requests_do_not_consume_capacity() {
    let Some(mut conn) = connect().await else {
        return;
    };
    let limiter = GcraLimiter::new();
    let key = unique_key("no-consume-on-reject");
    let now = now_ms();

    let first = limiter
        .check(&mut conn, &key, 60, 1, 60, now)
        .await
        .unwrap();
    assert!(first.allowed);

    // Several rapid, rejected retries at the same instant.
    for _ in 0..5 {
        let d = limiter
            .check(&mut conn, &key, 60, 1, 60, now)
            .await
            .unwrap();
        assert!(!d.allowed);
    }

    // One emission interval later (1000ms for 60/min), exactly one more
    // request should be admitted — proving the rejections above never ate
    // into capacity.
    let later = now + 1000;
    let admitted = limiter
        .check(&mut conn, &key, 60, 1, 60, later)
        .await
        .unwrap();
    assert!(admitted.allowed);
    let rejected_again = limiter
        .check(&mut conn, &key, 60, 1, 60, later)
        .await
        .unwrap();
    assert!(!rejected_again.allowed);
}
