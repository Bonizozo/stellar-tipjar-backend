/// Startup-failure integration tests for `AppConfig` (issue #339).
///
/// These tests verify that booting with a misconfigured environment:
///   1. Does NOT panic (no `expect`/`unwrap` abort)
///   2. Returns a clean, aggregated `ConfigError` listing every problem
///   3. The error message is human-readable (not a Rust backtrace)
///
/// Tests run in a subprocess-style fashion: each test temporarily mutates
/// the process environment, calls `AppConfig::from_env()`, then restores
/// the environment.  Tests are serialised with a Mutex to prevent races.
use std::sync::Mutex;
use stellar_tipjar_backend::config::AppConfig;

/// Ensures env-manipulation tests don't interfere with each other when
/// the test harness runs them in parallel threads.
static ENV_LOCK: Mutex<()> = Mutex::new(());

// ─────────────────────────── helpers ────────────────────────────────────────

fn clear_required_vars() {
    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("JWT_SECRET");
}

fn set_valid_config() {
    std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/tipjar_test");
    std::env::set_var(
        "JWT_SECRET",
        "a-sufficiently-long-and-random-test-secret-xyz!@#",
    );
}

// ─────────────────────────── tests ──────────────────────────────────────────

/// Missing `JWT_SECRET` must not panic — it must return a clean `ConfigError`
/// whose `Display` contains the variable name and a human-readable description.
#[test]
fn boot_fails_cleanly_when_jwt_secret_missing() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    clear_required_vars();
    std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/tipjar_test");
    // JWT_SECRET intentionally NOT set

    let result = AppConfig::from_env();

    let err = result.expect_err("should fail without JWT_SECRET");

    // The error must be a ConfigError with human-readable issues — not a panic.
    let display = format!("{err}");
    assert!(
        display.contains("JWT_SECRET"),
        "Error message must mention JWT_SECRET.\nActual:\n{display}"
    );

    // It must look like a configuration report, not a Rust backtrace.
    assert!(
        !display.contains("panicked at"),
        "Error must not be a panic backtrace.\nActual:\n{display}"
    );
    assert!(
        !display.contains("RUST_BACKTRACE"),
        "Error must not reference RUST_BACKTRACE.\nActual:\n{display}"
    );
}

/// Missing `DATABASE_URL` must produce a clean error mentioning DATABASE_URL.
#[test]
fn boot_fails_cleanly_when_database_url_missing() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    clear_required_vars();
    std::env::set_var(
        "JWT_SECRET",
        "a-sufficiently-long-and-random-test-secret-xyz!@#",
    );
    // DATABASE_URL intentionally NOT set

    let err = AppConfig::from_env().expect_err("should fail without DATABASE_URL");
    let display = format!("{err}");

    assert!(
        display.contains("DATABASE_URL"),
        "Error must mention DATABASE_URL.\nActual:\n{display}"
    );
    assert!(
        !display.contains("panicked at"),
        "Error must not be a panic.\nActual:\n{display}"
    );
}

/// Aggregated error: both required vars missing should list BOTH problems,
/// not just the first one (fail-fast with complete report).
#[test]
fn boot_error_is_aggregated_not_first_only() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    clear_required_vars();
    // Neither DATABASE_URL nor JWT_SECRET is set.

    let err = AppConfig::from_env().expect_err("should fail with two missing vars");

    assert!(
        err.issues.len() >= 2,
        "Should report at least 2 issues (DATABASE_URL + JWT_SECRET), got {}:\n{:?}",
        err.issues.len(),
        err.issues,
    );

    let display = format!("{err}");
    assert!(display.contains("DATABASE_URL"), "Must mention DATABASE_URL");
    assert!(display.contains("JWT_SECRET"), "Must mention JWT_SECRET");
}

/// A weak JWT secret (short, low-entropy) must be rejected with a descriptive
/// error even when the variable is present.
#[test]
fn boot_fails_on_weak_jwt_secret() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    clear_required_vars();
    std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/tipjar_test");
    std::env::set_var("JWT_SECRET", "weak"); // too short and low-entropy

    let err = AppConfig::from_env().expect_err("should reject weak JWT secret");
    let display = format!("{err}");

    assert!(
        display.contains("JWT_SECRET"),
        "Error must mention JWT_SECRET.\nActual:\n{display}"
    );
    // Must explain WHY it's invalid (length or entropy).
    assert!(
        display.contains("short") || display.contains("entropy"),
        "Error must explain why JWT_SECRET is invalid.\nActual:\n{display}"
    );
}

/// A valid configuration must load without any errors.
#[test]
fn boot_succeeds_with_valid_config() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    clear_required_vars();
    set_valid_config();

    let cfg = AppConfig::from_env().expect("valid config should succeed");

    assert!(!cfg.database.url.is_empty());
    assert!(!cfg.jwt.secret.is_empty());
    assert_eq!(cfg.port, 8000); // default
    assert_eq!(cfg.redis.url, "redis://127.0.0.1:6379"); // default
}

/// The error `Display` impl must produce a human-readable bullet list,
/// not a raw Rust debug dump.
#[test]
fn error_display_is_human_readable() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    clear_required_vars();
    // Set nothing — multiple errors expected

    let err = AppConfig::from_env().expect_err("should have errors");
    let display = format!("{err}");

    // Check structure: should start with a header line and list items with "•"
    assert!(
        display.contains("invalid") || display.contains("configuration"),
        "Display should have a readable header.\nActual:\n{display}"
    );
    assert!(
        display.contains('•'),
        "Display should use bullet points.\nActual:\n{display}"
    );
}
