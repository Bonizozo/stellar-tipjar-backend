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

/// Default number of consecutive failures before the circuit trips open.
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 5;

/// Default cooldown, in seconds, before an open circuit becomes half-open.
pub const DEFAULT_RECOVERY_TIMEOUT_SECS: u64 = 60;

/// Tunable circuit-breaker parameters.
///
/// Built either from code ([`CircuitBreakerConfig::default`]) or from the
/// environment ([`CircuitBreakerConfig::from_env`] /
/// [`CircuitBreakerConfig::from_env_prefixed`]) so thresholds can be adjusted
/// during an incident without a redeploy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before tripping open.
    pub failure_threshold: u32,
    /// How long the circuit stays open before transitioning to half-open.
    pub recovery_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            recovery_timeout: Duration::from_secs(DEFAULT_RECOVERY_TIMEOUT_SECS),
        }
    }
}

impl CircuitBreakerConfig {
    /// Read the global settings `CIRCUIT_BREAKER_FAILURE_THRESHOLD` and
    /// `CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS`, falling back to
    /// [`DEFAULT_FAILURE_THRESHOLD`] / [`DEFAULT_RECOVERY_TIMEOUT_SECS`].
    ///
    /// Unset, unparsable, or out-of-range values fall back to the defaults —
    /// a bad env var never prevents startup.
    pub fn from_env() -> Self {
        Self::resolve(None)
    }

    /// Same as [`CircuitBreakerConfig::from_env`], but a dependency-specific
    /// prefix takes precedence over the global variables. For `prefix = "DB"`
    /// the lookup order is:
    ///
    /// 1. `DB_CIRCUIT_BREAKER_FAILURE_THRESHOLD` / `DB_CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS`
    /// 2. `CIRCUIT_BREAKER_FAILURE_THRESHOLD` / `CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS`
    /// 3. the compiled-in defaults
    ///
    /// Each field resolves independently, so a prefixed threshold can be
    /// combined with a globally configured recovery window.
    pub fn from_env_prefixed(prefix: &str) -> Self {
        Self::resolve(Some(prefix))
    }

    fn resolve(prefix: Option<&str>) -> Self {
        // A threshold of 0 would report the circuit open before a single
        // request was made, so it is rejected like any other bad value.
        let failure_threshold =
            read_env_var(prefix, "CIRCUIT_BREAKER_FAILURE_THRESHOLD", |&threshold| {
                threshold > 0
            })
            .unwrap_or(DEFAULT_FAILURE_THRESHOLD);

        let recovery_secs = read_env_var(
            prefix,
            "CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS",
            // Zero is meaningful here: an open circuit probes immediately.
            |_: &u64| true,
        )
        .unwrap_or(DEFAULT_RECOVERY_TIMEOUT_SECS);

        let config = Self {
            failure_threshold,
            recovery_timeout: Duration::from_secs(recovery_secs),
        };

        tracing::debug!(
            prefix = prefix.unwrap_or("<global>"),
            failure_threshold = config.failure_threshold,
            recovery_timeout_secs = recovery_secs,
            "circuit breaker configured"
        );

        config
    }
}

/// Look up `name`, preferring `<PREFIX>_<name>` when a prefix is given.
///
/// Values that fail to parse or fail `is_valid` are logged and treated as
/// absent, so a bad prefixed value still falls through to the global one and,
/// failing that, to the caller's default.
fn read_env_var<T, F>(prefix: Option<&str>, name: &str, is_valid: F) -> Option<T>
where
    T: std::str::FromStr + std::fmt::Display,
    F: Fn(&T) -> bool,
{
    let keys = prefix
        .map(|p| format!("{}_{}", p.to_uppercase(), name))
        .into_iter()
        .chain(std::iter::once(name.to_string()));

    for key in keys {
        let Ok(raw) = std::env::var(&key) else {
            continue;
        };
        match raw.trim().parse::<T>() {
            Ok(value) if is_valid(&value) => return Some(value),
            Ok(value) => tracing::warn!(
                env_var = %key,
                value = %value,
                "circuit breaker setting out of range; ignoring"
            ),
            Err(_) => tracing::warn!(
                env_var = %key,
                value = %raw,
                "invalid circuit breaker setting; ignoring"
            ),
        }
    }
    None
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

    /// Build a breaker from an already-resolved [`CircuitBreakerConfig`].
    pub fn from_config(config: CircuitBreakerConfig) -> Self {
        Self::new(config.failure_threshold, config.recovery_timeout)
    }

    /// Build a breaker from the global `CIRCUIT_BREAKER_*` environment
    /// variables, falling back to the compiled-in defaults.
    pub fn from_env() -> Self {
        Self::from_config(CircuitBreakerConfig::from_env())
    }

    /// Build a breaker from the `<PREFIX>_CIRCUIT_BREAKER_*` environment
    /// variables, falling back to the global variables and then the defaults.
    /// See [`CircuitBreakerConfig::from_env_prefixed`].
    pub fn from_env_prefixed(prefix: &str) -> Self {
        Self::from_config(CircuitBreakerConfig::from_env_prefixed(prefix))
    }

    /// The failure threshold this breaker was built with.
    pub fn failure_threshold(&self) -> u32 {
        self.failure_threshold
    }

    /// The recovery window this breaker was built with.
    pub fn recovery_timeout(&self) -> Duration {
        self.recovery_timeout
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

impl Default for CircuitBreaker {
    /// A breaker with the compiled-in defaults, ignoring the environment.
    /// Prefer [`CircuitBreaker::from_env`] in production code paths.
    fn default() -> Self {
        Self::from_config(CircuitBreakerConfig::default())
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
    use std::sync::Mutex;

    /// `std::env` is process-global; serialize the env-reading tests so they
    /// cannot observe one another's variables.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ENV_KEYS: [&str; 4] = [
        "CIRCUIT_BREAKER_FAILURE_THRESHOLD",
        "CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS",
        "DB_CIRCUIT_BREAKER_FAILURE_THRESHOLD",
        "DB_CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS",
    ];

    fn clear_env() {
        for key in ENV_KEYS {
            std::env::remove_var(key);
        }
    }

    /// Run `body` with a clean circuit-breaker environment, restoring it after.
    fn with_clean_env(body: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_env();
        body();
        clear_env();
    }

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

    #[test]
    fn config_default_matches_documented_values() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.recovery_timeout, Duration::from_secs(60));

        let cb = CircuitBreaker::default();
        assert_eq!(cb.failure_threshold(), 5);
        assert_eq!(cb.recovery_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn config_from_env_falls_back_to_defaults_when_unset() {
        with_clean_env(|| {
            assert_eq!(CircuitBreakerConfig::from_env(), Default::default());
        });
    }

    #[test]
    fn config_from_env_reads_global_vars() {
        with_clean_env(|| {
            std::env::set_var("CIRCUIT_BREAKER_FAILURE_THRESHOLD", "12");
            std::env::set_var("CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS", "5");

            let config = CircuitBreakerConfig::from_env();
            assert_eq!(config.failure_threshold, 12);
            assert_eq!(config.recovery_timeout, Duration::from_secs(5));

            let cb = CircuitBreaker::from_env();
            assert_eq!(cb.failure_threshold(), 12);
        });
    }

    #[test]
    fn prefixed_vars_take_precedence_over_global_vars() {
        with_clean_env(|| {
            std::env::set_var("CIRCUIT_BREAKER_FAILURE_THRESHOLD", "12");
            std::env::set_var("DB_CIRCUIT_BREAKER_FAILURE_THRESHOLD", "20");
            // Recovery window is only set globally — it must still be picked up.
            std::env::set_var("CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS", "90");

            let config = CircuitBreakerConfig::from_env_prefixed("DB");
            assert_eq!(config.failure_threshold, 20);
            assert_eq!(config.recovery_timeout, Duration::from_secs(90));
        });
    }

    #[test]
    fn prefixed_lookup_is_case_insensitive() {
        with_clean_env(|| {
            std::env::set_var("DB_CIRCUIT_BREAKER_FAILURE_THRESHOLD", "7");
            assert_eq!(
                CircuitBreakerConfig::from_env_prefixed("db").failure_threshold,
                7
            );
        });
    }

    #[test]
    fn invalid_values_fall_back_without_panicking() {
        with_clean_env(|| {
            std::env::set_var("CIRCUIT_BREAKER_FAILURE_THRESHOLD", "not-a-number");
            std::env::set_var("CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS", "-1");

            assert_eq!(CircuitBreakerConfig::from_env(), Default::default());
        });
    }

    #[test]
    fn invalid_prefixed_value_falls_back_to_global_value() {
        with_clean_env(|| {
            std::env::set_var("DB_CIRCUIT_BREAKER_FAILURE_THRESHOLD", "garbage");
            std::env::set_var("CIRCUIT_BREAKER_FAILURE_THRESHOLD", "9");

            assert_eq!(
                CircuitBreakerConfig::from_env_prefixed("DB").failure_threshold,
                9
            );
        });
    }

    #[test]
    fn zero_threshold_is_rejected() {
        with_clean_env(|| {
            // A threshold of 0 would report the circuit open before any request
            // was ever made, so it is treated as invalid.
            std::env::set_var("CIRCUIT_BREAKER_FAILURE_THRESHOLD", "0");
            assert_eq!(
                CircuitBreakerConfig::from_env().failure_threshold,
                DEFAULT_FAILURE_THRESHOLD
            );
        });
    }

    #[test]
    fn zero_prefixed_threshold_falls_back_to_global_value() {
        with_clean_env(|| {
            std::env::set_var("DB_CIRCUIT_BREAKER_FAILURE_THRESHOLD", "0");
            std::env::set_var("CIRCUIT_BREAKER_FAILURE_THRESHOLD", "8");
            assert_eq!(
                CircuitBreakerConfig::from_env_prefixed("DB").failure_threshold,
                8
            );
        });
    }

    #[test]
    fn zero_recovery_timeout_is_allowed() {
        with_clean_env(|| {
            std::env::set_var("CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS", "0");
            assert_eq!(
                CircuitBreakerConfig::from_env().recovery_timeout,
                Duration::from_secs(0)
            );
        });
    }

    #[test]
    fn env_configured_breaker_trips_at_configured_threshold() {
        with_clean_env(|| {
            std::env::set_var("HORIZON_CIRCUIT_BREAKER_FAILURE_THRESHOLD", "2");
            let cb = CircuitBreaker::from_env_prefixed("HORIZON");
            std::env::remove_var("HORIZON_CIRCUIT_BREAKER_FAILURE_THRESHOLD");

            cb.record_failure();
            assert_eq!(cb.state(), CircuitState::Closed);
            cb.record_failure();
            assert_eq!(cb.state(), CircuitState::Open);
        });
    }
}
