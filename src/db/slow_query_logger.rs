use std::time::Duration;
use tracing::warn;

/// Threshold above which a query is considered slow.
const SLOW_QUERY_THRESHOLD_MS: u128 = 200;

pub struct SlowQueryLogger {
    threshold: Duration,
}

impl SlowQueryLogger {
    pub fn new() -> Self {
        Self {
            threshold: Duration::from_millis(SLOW_QUERY_THRESHOLD_MS as u64),
        }
    }

    pub fn with_threshold(threshold: Duration) -> Self {
        Self { threshold }
    }

    /// Log the query if it exceeds the threshold.
    ///
    /// Emits a structured `warn!` inside the *current* active span so the
    /// `tracing-opentelemetry` bridge converts it to a span event.  This makes
    /// slow queries visible in the trace waterfall alongside the HTTP/job span
    /// that triggered them, achieving log/trace correlation without requiring
    /// the OpenTelemetry API directly.  Returns true if the query was slow.
    pub fn check(&self, query: &str, duration: Duration) -> bool {
        if duration >= self.threshold {
            let trimmed = query.trim().replace('\n', " ");
            let duration_ms = duration.as_millis();

            // Emit inside whatever span is currently active.  The
            // tracing-opentelemetry layer turns this into an OTel span event
            // with `db.statement.summary` and `db.query.duration_ms` fields,
            // joining it to the parent HTTP or job span in the trace waterfall.
            warn!(
                target: "slow_query",
                duration_ms,
                db.statement.summary = %trimmed,
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
