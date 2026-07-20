//! Typed, validated application configuration.
//!
//! `AppConfig::from_env()` reads **all** environment variables once at startup,
//! collects every validation error, and returns an aggregated `ConfigError`
//! listing every missing/invalid variable rather than stopping at the first.
//!
//! After a successful load the struct is threaded through [`AppState`] so that
//! no other code ever calls `std::env::var` directly.
//!
//! # Environment Variables
//!
//! | Variable                        | Required | Default                                     |
//! |---------------------------------|----------|---------------------------------------------|
//! | `DATABASE_URL`                  | yes      | —                                           |
//! | `REDIS_URL`                     | no       | `redis://127.0.0.1:6379`                    |
//! | `JWT_SECRET`                    | yes      | — (min 32 bytes, entropy checked)           |
//! | `SMTP_HOST`                     | no       | `localhost`                                 |
//! | `SMTP_PORT`                     | no       | `587`                                       |
//! | `SMTP_USER`                     | no       | —                                           |
//! | `SMTP_PASS`                     | no       | —                                           |
//! | `SMTP_FROM`                     | no       | `no-reply@stellar-tipjar.com`               |
//! | `ALLOWED_ORIGINS`               | no       | `*`                                         |
//! | `CORS_MAX_AGE_SECS`             | no       | `3600`                                      |
//! | `RATE_LIMIT_PER_SECOND`         | no       | `10`                                        |
//! | `RATE_LIMIT_BURST_SIZE`         | no       | `20`                                        |
//! | `RATE_LIMIT_WRITE_PER_SECOND`   | no       | `2`                                         |
//! | `RATE_LIMIT_WRITE_BURST_SIZE`   | no       | `5`                                         |
//! | `RATE_LIMIT_WHITELIST`          | no       | `""`                                        |
//! | `WEBHOOK_SECRET`                | no       | —                                           |
//! | `STELLAR_RPC_URL`               | no       | `https://soroban-testnet.stellar.org`       |
//! | `STELLAR_NETWORK`               | no       | `testnet`                                   |
//! | `PORT`                          | no       | `8000`                                      |
//! | `REQUEST_TIMEOUT_SECS`          | no       | `30`                                        |
//! | `PAGINATION_MAX_OFFSET`         | no       | `10000`                                     |
//! | `PAGINATION_CURSOR_SECRET`      | no       | falls back to `JWT_SECRET`                  |
//! | `TIP_RATE_LIMIT_PER_MINUTE`     | no       | `10`                                        |
//! | `MIN_TIP_AMOUNT`                | no       | `0.01`                                      |
//! | `MAX_TIP_AMOUNT`                | no       | `10000`                                     |

use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

/// Aggregated configuration error listing every invalid/missing variable.
#[derive(Debug, thiserror::Error)]
#[error("Application configuration is invalid:\n{}", .issues.iter().map(|s| format!("  • {s}")).collect::<Vec<_>>().join("\n"))]
pub struct ConfigError {
    pub issues: Vec<String>,
}

impl ConfigError {
    fn new(issues: Vec<String>) -> Self {
        Self { issues }
    }
}

// ─────────────────────────── Sub-configs ────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Raw secret bytes — stored as `String` so it can be passed to
    /// `EncodingKey::from_secret` / `DecodingKey::from_secret`.
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub pass: Option<String>,
    pub from: String,
}

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub max_age_secs: u64,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub general_per_second: u64,
    pub general_burst_size: u32,
    pub write_per_second: u64,
    pub write_burst_size: u32,
    /// Parsed set of whitelisted IPs; entries that fail to parse are ignored.
    pub whitelist: Vec<IpAddr>,
}

#[derive(Debug, Clone)]
pub struct StellarConfig {
    pub rpc_url: String,
    pub network: String,
}

#[derive(Debug, Clone)]
pub struct PaginationConfig {
    pub max_offset: i64,
    /// HMAC signing key for cursor tokens.  Defaults to `jwt.secret`.
    pub cursor_secret: String,
}

#[derive(Debug, Clone)]
pub struct TipValidationConfig {
    pub min_amount: rust_decimal::Decimal,
    pub max_amount: rust_decimal::Decimal,
    pub rate_limit_per_minute: i64,
}

// ─────────────────────────── Root config ────────────────────────────────────

/// Complete application configuration, loaded once at startup.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub jwt: JwtConfig,
    pub smtp: SmtpConfig,
    pub cors: CorsConfig,
    pub rate_limit: RateLimitConfig,
    pub stellar: StellarConfig,
    pub pagination: PaginationConfig,
    pub tip_validation: TipValidationConfig,
    /// Optional webhook HMAC secret; `None` when `WEBHOOK_SECRET` is unset.
    pub webhook_secret: Option<String>,
    /// HTTP server port.
    pub port: u16,
    /// Per-request timeout.
    pub request_timeout: Duration,
}

// ─────────────────────────── Parser helpers ─────────────────────────────────

struct EnvReader {
    issues: Vec<String>,
}

impl EnvReader {
    fn new() -> Self {
        Self { issues: Vec::new() }
    }

    /// Return the variable's value or record an error and return `None`.
    fn require(&mut self, key: &str) -> Option<String> {
        match std::env::var(key) {
            Ok(v) if !v.trim().is_empty() => Some(v),
            Ok(_) => {
                self.issues
                    .push(format!("{key}: is set but empty"));
                None
            }
            Err(_) => {
                self.issues.push(format!("{key}: not set (required)"));
                None
            }
        }
    }

    /// Return the variable's value or a default; never records an error.
    fn optional(&self, key: &str, default: &str) -> String {
        std::env::var(key)
            .unwrap_or_else(|_| default.to_string())
    }

    /// Parse an optional environment variable as `T`; records an error on
    /// parse failure and returns the default.
    fn parse_or<T>(&mut self, key: &str, default: T) -> T
    where
        T: FromStr + Copy,
        T::Err: std::fmt::Display,
    {
        match std::env::var(key) {
            Err(_) => default,
            Ok(v) => v.parse::<T>().unwrap_or_else(|e| {
                self.issues
                    .push(format!("{key}: invalid value ({e})"));
                default
            }),
        }
    }

    /// Parse an optional variable that may be `None`; records an error on
    /// parse failure.
    fn parse_opt<T>(&mut self, key: &str) -> Option<T>
    where
        T: FromStr,
        T::Err: std::fmt::Display,
    {
        std::env::var(key).ok().and_then(|v| {
            if v.is_empty() {
                None
            } else {
                v.parse::<T>()
                    .map_err(|e| {
                        self.issues
                            .push(format!("{key}: invalid value ({e})"));
                    })
                    .ok()
            }
        })
    }

    fn finish(self) -> Vec<String> {
        self.issues
    }
}

// ─────────────────────────── JWT entropy check ──────────────────────────────

/// Minimum required byte length for the JWT secret.
const JWT_SECRET_MIN_BYTES: usize = 32;

/// Minimum required Shannon entropy (bits per byte).  A random 32-byte secret
/// has ≈ 8 bits/byte; an all-zeros secret has 0 bits/byte.  We reject anything
/// below 3.5 bits/byte which catches obvious weak secrets like "password".
const JWT_SECRET_MIN_ENTROPY: f64 = 3.5;

fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let bytes = s.as_bytes();
    let len = bytes.len() as f64;
    let mut counts = [0u32; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

fn validate_jwt_secret(secret: &str, issues: &mut Vec<String>) {
    if secret.len() < JWT_SECRET_MIN_BYTES {
        issues.push(format!(
            "JWT_SECRET: too short ({} bytes, minimum {})",
            secret.len(),
            JWT_SECRET_MIN_BYTES
        ));
    }
    let entropy = shannon_entropy(secret);
    if entropy < JWT_SECRET_MIN_ENTROPY {
        issues.push(format!(
            "JWT_SECRET: entropy too low ({entropy:.2} bits/byte, minimum {JWT_SECRET_MIN_ENTROPY}). \
             Use a randomly generated secret."
        ));
    }
}

// ─────────────────────────── AppConfig::from_env ────────────────────────────

impl AppConfig {
    /// Read and validate all configuration from the environment.
    ///
    /// Collects **all** validation errors before returning so that a
    /// misconfigured deployment surfaces every problem at once.
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut reader = EnvReader::new();
        let mut extra_issues: Vec<String> = Vec::new();

        // ── Required ─────────────────────────────────────────────────────────
        let database_url = reader.require("DATABASE_URL");
        let jwt_secret_raw = reader.require("JWT_SECRET");

        // JWT entropy validation (only when the key was present)
        if let Some(ref secret) = jwt_secret_raw {
            validate_jwt_secret(secret, &mut extra_issues);
        }

        // ── Optional with defaults ────────────────────────────────────────────
        let redis_url = reader.optional("REDIS_URL", "redis://127.0.0.1:6379");

        let smtp_host = reader.optional("SMTP_HOST", "localhost");
        let smtp_port: u16 = reader.parse_or("SMTP_PORT", 587);
        let smtp_user = std::env::var("SMTP_USER").ok().filter(|v| !v.is_empty());
        let smtp_pass = std::env::var("SMTP_PASS").ok().filter(|v| !v.is_empty());
        let smtp_from = reader.optional("SMTP_FROM", "no-reply@stellar-tipjar.com");

        let cors_origins_raw = std::env::var("ALLOWED_ORIGINS")
            .or_else(|_| std::env::var("CORS_ALLOWED_ORIGINS"))
            .unwrap_or_default();
        let cors_allowed_origins: Vec<String> = cors_origins_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let cors_max_age_secs: u64 = reader.parse_or("CORS_MAX_AGE_SECS", 3600);

        let rl_per_second: u64 = reader.parse_or("RATE_LIMIT_PER_SECOND", 10);
        let rl_burst: u32 = reader.parse_or("RATE_LIMIT_BURST_SIZE", 20);
        let rl_write_per_second: u64 = reader.parse_or("RATE_LIMIT_WRITE_PER_SECOND", 2);
        let rl_write_burst: u32 = reader.parse_or("RATE_LIMIT_WRITE_BURST_SIZE", 5);
        let rl_whitelist: Vec<IpAddr> = std::env::var("RATE_LIMIT_WHITELIST")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        let stellar_rpc = reader.optional(
            "STELLAR_RPC_URL",
            "https://soroban-testnet.stellar.org",
        );
        let stellar_network = reader.optional("STELLAR_NETWORK", "testnet");

        let pagination_max_offset: i64 = reader.parse_or("PAGINATION_MAX_OFFSET", 10_000);
        // cursor_secret falls back to JWT_SECRET; if JWT_SECRET is missing we
        // insert a placeholder so later code can still construct the struct.
        let pagination_cursor_secret = std::env::var("PAGINATION_CURSOR_SECRET")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var("JWT_SECRET").ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "development-pagination-secret-change-me".to_string());

        let tip_min: rust_decimal::Decimal = std::env::var("MIN_TIP_AMOUNT")
            .or_else(|_| std::env::var("TIP_MIN_XLM"))
            .ok()
            .and_then(|v| rust_decimal::Decimal::from_str(&v).ok())
            // SAFETY: "0.01" is a compile-time string literal that is always valid.
            // Invariant: Decimal::from_str on a known-good literal cannot fail.
            .unwrap_or_else(|| {
                #[allow(clippy::expect_used)]
                rust_decimal::Decimal::from_str("0.01")
                    .expect("valid decimal literal: 0.01")
            });
        let tip_max: rust_decimal::Decimal = std::env::var("MAX_TIP_AMOUNT")
            .or_else(|_| std::env::var("TIP_MAX_XLM"))
            .ok()
            .and_then(|v| rust_decimal::Decimal::from_str(&v).ok())
            // SAFETY: "10000" is a compile-time string literal that is always valid.
            // Invariant: Decimal::from_str on a known-good literal cannot fail.
            .unwrap_or_else(|| {
                #[allow(clippy::expect_used)]
                rust_decimal::Decimal::from_str("10000")
                    .expect("valid decimal literal: 10000")
            });
        let tip_rpm: i64 = reader.parse_or("TIP_RATE_LIMIT_PER_MINUTE", 10);

        let webhook_secret = std::env::var("WEBHOOK_SECRET")
            .ok()
            .filter(|v| !v.is_empty());

        let port: u16 = reader.parse_or("PORT", 8000);
        let timeout_secs: u64 = reader.parse_or("REQUEST_TIMEOUT_SECS", 30);

        // ── Collect all errors ────────────────────────────────────────────────
        let mut all_issues = reader.finish();
        all_issues.extend(extra_issues);

        if !all_issues.is_empty() {
            return Err(ConfigError::new(all_issues));
        }

        // At this point all required fields are guaranteed to be Some.
        // SAFETY: `database_url` and `jwt_secret` are set by `reader.require()`
        // above.  If either was missing, `all_issues` would be non-empty and we
        // would have returned `Err` before reaching this line.
        // Invariant: required fields are `Some` when `all_issues` is empty.
        #[allow(clippy::expect_used)]
        let database_url = database_url.expect("checked above: database_url is Some when issues is empty");
        #[allow(clippy::expect_used)]
        let jwt_secret = jwt_secret_raw.expect("checked above: jwt_secret is Some when issues is empty");

        Ok(AppConfig {
            database: DatabaseConfig { url: database_url },
            redis: RedisConfig { url: redis_url },
            jwt: JwtConfig { secret: jwt_secret },
            smtp: SmtpConfig {
                host: smtp_host,
                port: smtp_port,
                user: smtp_user,
                pass: smtp_pass,
                from: smtp_from,
            },
            cors: CorsConfig {
                allowed_origins: cors_allowed_origins,
                max_age_secs: cors_max_age_secs,
            },
            rate_limit: RateLimitConfig {
                general_per_second: rl_per_second,
                general_burst_size: rl_burst,
                write_per_second: rl_write_per_second,
                write_burst_size: rl_write_burst,
                whitelist: rl_whitelist,
            },
            stellar: StellarConfig {
                rpc_url: stellar_rpc,
                network: stellar_network,
            },
            pagination: PaginationConfig {
                max_offset: pagination_max_offset,
                cursor_secret: pagination_cursor_secret,
            },
            tip_validation: TipValidationConfig {
                min_amount: tip_min,
                max_amount: tip_max,
                rate_limit_per_minute: tip_rpm,
            },
            webhook_secret,
            port,
            request_timeout: Duration::from_secs(timeout_secs),
        })
    }
}

// ─────────────────────────── Tests ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn set_minimal_env() {
        std::env::set_var("DATABASE_URL", "postgres://test/test");
        std::env::set_var(
            "JWT_SECRET",
            "a-sufficiently-long-and-entropic-test-secret-xyz!",
        );
    }

    fn clear_env() {
        for key in &[
            "DATABASE_URL",
            "JWT_SECRET",
            "REDIS_URL",
            "SMTP_HOST",
            "SMTP_PORT",
            "SMTP_FROM",
            "PORT",
            "REQUEST_TIMEOUT_SECS",
            "RATE_LIMIT_PER_SECOND",
            "ALLOWED_ORIGINS",
            "STELLAR_RPC_URL",
            "PAGINATION_MAX_OFFSET",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn succeeds_with_minimal_config() {
        clear_env();
        set_minimal_env();
        let cfg = AppConfig::from_env().expect("should succeed");
        assert_eq!(cfg.database.url, "postgres://test/test");
        assert_eq!(cfg.port, 8000);
        assert_eq!(cfg.redis.url, "redis://127.0.0.1:6379");
        clear_env();
    }

    #[test]
    fn fails_when_database_url_missing() {
        clear_env();
        std::env::set_var(
            "JWT_SECRET",
            "a-sufficiently-long-and-entropic-test-secret-xyz!",
        );
        std::env::remove_var("DATABASE_URL");
        let err = AppConfig::from_env().expect_err("should fail");
        assert!(
            err.issues.iter().any(|e| e.contains("DATABASE_URL")),
            "error should mention DATABASE_URL: {err}"
        );
        clear_env();
    }

    #[test]
    fn fails_when_jwt_secret_missing() {
        clear_env();
        std::env::set_var("DATABASE_URL", "postgres://test/test");
        std::env::remove_var("JWT_SECRET");
        let err = AppConfig::from_env().expect_err("should fail");
        assert!(
            err.issues.iter().any(|e| e.contains("JWT_SECRET")),
            "error should mention JWT_SECRET: {err}"
        );
        clear_env();
    }

    #[test]
    fn fails_when_jwt_secret_too_short() {
        clear_env();
        std::env::set_var("DATABASE_URL", "postgres://test/test");
        std::env::set_var("JWT_SECRET", "short");
        let err = AppConfig::from_env().expect_err("should fail");
        assert!(
            err.issues.iter().any(|e| e.contains("too short")),
            "error should mention too short: {err}"
        );
        clear_env();
    }

    #[test]
    fn fails_when_jwt_secret_low_entropy() {
        clear_env();
        std::env::set_var("DATABASE_URL", "postgres://test/test");
        // 32 bytes but all the same character → zero entropy
        std::env::set_var("JWT_SECRET", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let err = AppConfig::from_env().expect_err("should fail");
        assert!(
            err.issues.iter().any(|e| e.contains("entropy")),
            "error should mention entropy: {err}"
        );
        clear_env();
    }

    #[test]
    fn aggregates_multiple_errors() {
        clear_env();
        std::env::remove_var("DATABASE_URL");
        std::env::remove_var("JWT_SECRET");
        let err = AppConfig::from_env().expect_err("should fail");
        assert!(
            err.issues.len() >= 2,
            "should aggregate at least 2 errors, got: {err}"
        );
        clear_env();
    }

    #[test]
    fn entropy_calculation() {
        // High-entropy random-like string
        assert!(shannon_entropy("aB3$xY!2qW#9") > 3.5);
        // All same byte → zero entropy
        assert_eq!(shannon_entropy("aaaa"), 0.0);
        // Empty string
        assert_eq!(shannon_entropy(""), 0.0);
    }

    #[test]
    fn custom_port_and_timeout() {
        clear_env();
        set_minimal_env();
        std::env::set_var("PORT", "9090");
        std::env::set_var("REQUEST_TIMEOUT_SECS", "60");
        let cfg = AppConfig::from_env().expect("should succeed");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.request_timeout, Duration::from_secs(60));
        clear_env();
    }
}
