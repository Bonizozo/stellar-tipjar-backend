use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

/// Configuration for retry with exponential backoff.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (excluding the initial call).
    pub max_retries: u32,
    /// Base delay between retries. Actual delay = base * 2^attempt.
    pub base_delay: Duration,
    /// Maximum delay cap to prevent excessively long waits.
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
        }
    }
}

/// Execute an async operation with exponential backoff retry.
///
/// Retries on `Err` results up to `config.max_retries` times.
/// The delay between retries doubles each attempt, capped at `max_delay`.
pub async fn with_retry<F, Fut, T, E>(config: &RetryConfig, operation: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut last_err: Option<E> = None;

    for attempt in 0..=config.max_retries {
        match operation().await {
            Ok(value) => {
                if attempt > 0 {
                    tracing::info!("Operation succeeded on attempt {}", attempt + 1);
                }
                return Ok(value);
            }
            Err(err) => {
                if attempt < config.max_retries {
                    let delay = config
                        .base_delay
                        .saturating_mul(2u32.saturating_pow(attempt))
                        .min(config.max_delay);
                    tracing::warn!(
                        "Attempt {}/{} failed: {}. Retrying in {:?}",
                        attempt + 1,
                        config.max_retries + 1,
                        err,
                        delay,
                    );
                    sleep(delay).await;
                }
                last_err = Some(err);
            }
        }
    }

    Err(last_err.expect("at least one attempt was made"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_succeeds_on_first_try() {
        let config = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
        };

        let result: Result<&str, String> =
            with_retry(&config, || async { Ok("ok") }).await;
        assert_eq!(result.unwrap(), "ok");
    }

    #[tokio::test]
    async fn test_succeeds_after_retries() {
        let config = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result: Result<&str, String> = with_retry(&config, || {
            let c = counter_clone.clone();
            async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err("transient error".to_string())
                } else {
                    Ok("ok")
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "ok");
        assert_eq!(counter.load(Ordering::SeqCst), 3); // 2 failures + 1 success
    }

    #[tokio::test]
    async fn test_fails_after_max_retries() {
        let config = RetryConfig {
            max_retries: 2,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result: Result<&str, String> = with_retry(&config, || {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err("permanent error".to_string())
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "permanent error");
        assert_eq!(counter.load(Ordering::SeqCst), 3); // initial + 2 retries
    }
}
