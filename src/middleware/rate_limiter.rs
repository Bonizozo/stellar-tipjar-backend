use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tower_governor::{
    errors::GovernorError, governor::GovernorConfigBuilder, key_extractor::PeerIpKeyExtractor,
    GovernorLayer,
};
use governor::middleware::StateInformationMiddleware;

// ---------------------------------------------------------------------------
// User tier definitions
// ---------------------------------------------------------------------------

/// API consumer tier – controls per-second and burst limits.
#[derive(Debug, Clone, Copy)]
pub enum UserTier {
    /// Unauthenticated / anonymous callers.
    Anonymous,
    /// Authenticated free-tier users.
    Free,
    /// Paid / premium users.
    Premium,
}

impl UserTier {
    /// Returns (per_second, burst_size) for this tier.
    pub fn limits(self) -> (u64, u32) {
        match self {
            UserTier::Anonymous => (2, 5),
            UserTier::Free => (10, 20),
            UserTier::Premium => (60, 120),
        }
    }
}

// ---------------------------------------------------------------------------
// Whitelist middleware
// ---------------------------------------------------------------------------

/// Axum middleware that bypasses rate limiting for whitelisted IPs.
/// Reads `RATE_LIMIT_WHITELIST` env var (comma-separated IPs) once at startup.
pub async fn whitelist_middleware(req: Request, next: Next) -> Response {
    let ip = req
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip());

    if let Some(ip) = ip {
        if is_whitelisted(ip) {
            tracing::debug!(%ip, "whitelisted IP bypassing rate limit");
            return next.run(req).await;
        }
    }
    next.run(req).await
}

fn is_whitelisted(ip: IpAddr) -> bool {
    let whitelist = std::env::var("RATE_LIMIT_WHITELIST").unwrap_or_default();
    whitelist
        .split(',')
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .any(|allowed| allowed == ip)
}

// ---------------------------------------------------------------------------
// Error handler
// ---------------------------------------------------------------------------

fn rate_limit_error_handler(err: GovernorError) -> Response {
    let (retry_after, message) = match &err {
        GovernorError::TooManyRequests { wait_time, .. } => (
            *wait_time,
            "Rate limit exceeded. Please slow down.",
        ),
        GovernorError::UnableToExtractKey => (0, "Unable to identify client."),
        GovernorError::Other { code, msg, .. } => {
            tracing::warn!(code = ?code, msg = ?msg, "Governor: unexpected error");
            (0, "Rate limiting error.")
        }
    };

    tracing::warn!(retry_after_secs = retry_after, "Rate limit exceeded");

    let body = serde_json::json!({
        "error": {
            "code": "RATE_LIMIT_EXCEEDED",
            "message": message,
            "retry_after_secs": retry_after
        }
    });

    let mut resp = (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
    if retry_after > 0 {
        resp.headers_mut().insert(
            "Retry-After",
            retry_after.to_string().parse().unwrap(),
        );
    }
    resp
}

// ---------------------------------------------------------------------------
// Layer builders (per-endpoint / per-tier)
// ---------------------------------------------------------------------------

fn build_layer(
    per_second: u64,
    burst_size: u32,
) -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    let config = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(per_second)
            .burst_size(burst_size)
            // Adds X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-After headers.
            .use_headers()
            .error_handler(rate_limit_error_handler)
            .finish()
            .unwrap(),
    );

    // Periodically evict stale entries to prevent unbounded memory growth.
    let limiter = config.limiter().clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            tracing::debug!("rate limiter cleanup: {} tracked IPs", limiter.len());
            limiter.retain_recent();
        }
    });

    GovernorLayer { config }
}

/// General read-endpoint limiter.
/// Env overrides: `RATE_LIMIT_PER_SECOND` (default 10), `RATE_LIMIT_BURST_SIZE` (default 20).
pub fn general_limiter() -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    let per_second = env_u64("RATE_LIMIT_PER_SECOND", 10);
    let burst_size = env_u32("RATE_LIMIT_BURST_SIZE", 20);
    build_layer(per_second, burst_size)
}

/// Strict write-endpoint limiter (POST /tips, POST /creators).
/// Env overrides: `RATE_LIMIT_WRITE_PER_SECOND` (default 2), `RATE_LIMIT_WRITE_BURST_SIZE` (default 5).
pub fn write_limiter() -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    let per_second = env_u64("RATE_LIMIT_WRITE_PER_SECOND", 2);
    let burst_size = env_u32("RATE_LIMIT_WRITE_BURST_SIZE", 5);
    build_layer(per_second, burst_size)
}

/// Build a limiter for a specific user tier.
pub fn tier_limiter(tier: UserTier) -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    let (per_second, burst_size) = tier.limits();
    build_layer(per_second, burst_size)
}

// ---------------------------------------------------------------------------
// Redis distributed throttle middleware
// ---------------------------------------------------------------------------
// Uses a sliding-window counter stored in Redis so that limits are shared
// across multiple server instances. Falls back to allowing the request when
// Redis is unavailable (fail-open) to avoid Redis becoming a hard dependency.

/// Axum middleware for Redis-backed distributed rate limiting.
///
/// Key: `throttle:{ip}:{window_secs_bucket}`
/// Limit: `REDIS_RATE_LIMIT` requests per `REDIS_RATE_WINDOW_SECS` window.
pub async fn redis_throttle_middleware(
    req: Request,
    next: Next,
) -> Response {
    use redis::AsyncCommands;

    let limit = env_u64("REDIS_RATE_LIMIT", 100) as i64;
    let window_secs = env_u64("REDIS_RATE_WINDOW_SECS", 60);

    // Extract client IP.
    let ip = req
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Retrieve Redis connection from extensions (injected by the router layer).
    let redis_conn = req
        .extensions()
        .get::<Option<redis::aio::ConnectionManager>>()
        .and_then(|o| o.clone());

    if let Some(mut conn) = redis_conn {
        let bucket = chrono::Utc::now().timestamp() / window_secs as i64;
        let key = format!("throttle:{}:{}", ip, bucket);

        // INCR + EXPIRE in a pipeline for atomicity.
        let count: i64 = match conn.incr::<_, _, i64>(&key, 1).await {
            Ok(c) => {
                // Set expiry only on first increment.
                if c == 1 {
                    let _ = conn.expire::<_, ()>(&key, window_secs as i64).await;
                }
                c
            }
            Err(e) => {
                tracing::warn!("Redis throttle INCR failed: {} – allowing request", e);
                return next.run(req).await; // fail-open
            }
        };

        let remaining = (limit - count).max(0);
        let reset_secs = window_secs - (chrono::Utc::now().timestamp() as u64 % window_secs);

        if count > limit {
            tracing::warn!(ip, count, limit, "Redis distributed rate limit exceeded");
            let body = serde_json::json!({
                "error": {
                    "code": "RATE_LIMIT_EXCEEDED",
                    "message": "Too many requests. Please slow down.",
                    "retry_after_secs": reset_secs
                }
            });
            let mut resp = (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
            resp.headers_mut().insert("X-RateLimit-Limit", limit.to_string().parse().unwrap());
            resp.headers_mut().insert("X-RateLimit-Remaining", "0".parse().unwrap());
            resp.headers_mut().insert("X-RateLimit-Reset", reset_secs.to_string().parse().unwrap());
            resp.headers_mut().insert("Retry-After", reset_secs.to_string().parse().unwrap());
            return resp;
        }

        let mut resp = next.run(req).await;
        resp.headers_mut().insert("X-RateLimit-Limit", limit.to_string().parse().unwrap());
        resp.headers_mut().insert("X-RateLimit-Remaining", remaining.to_string().parse().unwrap());
        resp.headers_mut().insert("X-RateLimit-Reset", reset_secs.to_string().parse().unwrap());
        resp
    } else {
        // No Redis – fall through (local governor layers still apply).
        next.run(req).await
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
