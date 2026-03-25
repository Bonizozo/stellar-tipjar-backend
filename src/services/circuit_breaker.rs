use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation; requests are allowed.
    Closed,
    /// Too many failures; requests are rejected immediately.
    Open,
    /// Cooling off; a single probe request is allowed to test recovery.
    HalfOpen,
}

/// A thread-safe circuit breaker that trips after consecutive failures
/// and recovers after a cooldown period.
pub struct CircuitBreaker {
    /// Number of consecutive failures before tripping open.
    failure_threshold: u32,
    /// How long the circuit stays open before transitioning to half-open.
    recovery_timeout: Duration,
    /// Current consecutive failure count.
    failure_count: AtomicU32,
    /// Unix timestamp (seconds) when the circuit was last tripped open.
    last_failure_time: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            failure_count: AtomicU32::new(0),
            last_failure_time: AtomicU64::new(0),
        }
    }

    /// Return the current circuit state.
    pub fn state(&self) -> CircuitState {
        let failures = self.failure_count.load(Ordering::SeqCst);
        if failures < self.failure_threshold {
            return CircuitState::Closed;
        }

        let last_fail = self.last_failure_time.load(Ordering::SeqCst);
        let now = now_secs();

        if now - last_fail >= self.recovery_timeout.as_secs() {
            CircuitState::HalfOpen
        } else {
            CircuitState::Open
        }
    }

    /// Check whether a request should be allowed through.
    pub fn allow_request(&self) -> bool {
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true, // allow probe request
            CircuitState::Open => false,
        }
    }

    /// Record a successful operation. Resets the failure counter.
    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
    }

    /// Record a failed operation. Increments the failure counter and
    /// updates the last failure timestamp.
    pub fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::SeqCst);
        self.last_failure_time.store(now_secs(), Ordering::SeqCst);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starts_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure(); // hits threshold
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_resets_on_success() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        cb.record_failure();
        cb.record_failure();
        cb.record_success();

        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_half_open_after_timeout() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(0)); // 0 sec timeout for test
        cb.record_failure();
        cb.record_failure();

        // Recovery timeout is 0 seconds, so it should immediately be half-open
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.allow_request());
    }
}
