use std::time::Duration;
use tracing::warn;

/// Threshold above which a query is considered slow.
const SLOW_QUERY_THRESHOLD_MS: u128 = 200;

pub struct SlowQueryLogger {
    threshold: Duration,
}

impl SlowQueryLogger {
    pub fn new() -> Self {
        Self { threshold: Duration::from_millis(SLOW_QUERY_THRESHOLD_MS as u64) }
    }

    pub fn with_threshold(threshold: Duration) -> Self {
        Self { threshold }
    }

    /// Log the query if it exceeds the threshold. Returns true if it was slow.
    pub fn check(&self, query: &str, duration: Duration) -> bool {
        if duration >= self.threshold {
            warn!(
                target: "slow_query",
                duration_ms = duration.as_millis(),
                query = query.trim(),
                "Slow query detected"
            );
            return true;
        }
        false
    }
}

impl Default for SlowQueryLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slow_query_detected() {
        let logger = SlowQueryLogger::new();
        assert!(logger.check("SELECT * FROM tips", Duration::from_millis(500)));
    }

    #[test]
    fn test_fast_query_not_flagged() {
        let logger = SlowQueryLogger::new();
        assert!(!logger.check("SELECT 1", Duration::from_millis(10)));
    }

    #[test]
    fn test_custom_threshold() {
        let logger = SlowQueryLogger::with_threshold(Duration::from_millis(50));
        assert!(logger.check("SELECT 1", Duration::from_millis(51)));
        assert!(!logger.check("SELECT 1", Duration::from_millis(49)));
    }
}
