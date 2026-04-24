use std::future::Future;
use std::time::Duration;

use sqlx::PgPool;

/// Retry a database operation on deadlock or serialization failures.
///
/// Uses exponential backoff starting at 100 ms, doubling each attempt.
/// Non-retryable errors are returned immediately.
pub async fn with_db_retry<F, T, E>(
    pool: &PgPool,
    max_retries: u32,
    f: F,
) -> Result<T, E>
where
    F: Fn(&PgPool) -> std::pin::Pin<Box<dyn Future<Output = Result<T, E>> + Send>>,
    E: From<sqlx::Error> + std::fmt::Debug,
{
    let mut attempts = 0u32;

    loop {
        match f(pool).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempts += 1;
                if attempts > max_retries || !is_retryable(&e) {
                    return Err(e);
                }
                let delay = Duration::from_millis(100 * 2u64.saturating_pow(attempts - 1));
                tracing::warn!(
                    attempt = attempts,
                    max = max_retries,
                    error = ?e,
                    "Retryable DB error, backing off {:?}",
                    delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

fn is_retryable<E: std::fmt::Debug>(error: &E) -> bool {
    let s = format!("{:?}", error);
    s.contains("deadlock")
        || s.contains("serialization failure")
        || s.contains("could not serialize")
        || s.contains("40001") // PostgreSQL serialization_failure SQLSTATE
        || s.contains("40P01") // PostgreSQL deadlock_detected SQLSTATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn retries_on_retryable_error() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        // Simulate a pool — we won't actually connect, just test retry logic.
        // We use a dummy pool URL that will fail to connect; the closure never
        // uses the pool, so this is fine for unit-testing the retry loop.
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();

        let result: Result<(), String> = with_db_retry(&pool, 2, |_pool| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err("deadlock detected".to_string())
            })
        })
        .await;

        assert!(result.is_err());
        // initial attempt + 2 retries = 3 total
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_error() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();

        let result: Result<(), String> = with_db_retry(&pool, 3, |_pool| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err("unique constraint violation".to_string())
            })
        })
        .await;

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn succeeds_on_first_attempt() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap();

        let result: Result<u32, String> =
            with_db_retry(&pool, 3, |_pool| Box::pin(async { Ok(42u32) })).await;

        assert_eq!(result.unwrap(), 42);
    }
}
