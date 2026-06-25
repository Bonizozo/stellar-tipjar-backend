
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use redis::AsyncCommands;

use crate::{
    db::connection::AppState,
    errors::AppError,
    gateway::context::GatewayIdentity,
    metrics::collectors::{
        RATE_LIMIT_BURST_CONSUMED_TOTAL, RATE_LIMIT_EXCEEDED_TOTAL,
        RATE_LIMIT_REQUESTS_TOTAL,
    },
};

// ── Tier definitions ──────────────────────────────────────────────────────────

/// Caller tier that drives rate-limit thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerTier {
    /// Unauthenticated callers.
    Anonymous,
    /// Authenticated users on the free plan.
    Free,
    /// Authenticated premium / paid users.
    Premium,
    /// Internal admin callers — very high limits.
    Admin,
}

impl CallerTier {
    /// Returns `(requests_per_minute, burst_size)` for this tier.
    ///
    /// All values are overridable via env vars:
    /// - `RATE_LIMIT_ANON_RPM` / `RATE_LIMIT_ANON_BURST`
    /// - `RATE_LIMIT_FREE_RPM` / `RATE_LIMIT_FREE_BURST`
    /// - `RATE_LIMIT_PREMIUM_RPM` / `RATE_LIMIT_PREMIUM_BURST`
    /// - `RATE_LIMIT_ADMIN_RPM` / `RATE_LIMIT_ADMIN_BURST`
    pub fn limits(self) -> (u64, u64) {
        match self {
            CallerTier::Anonymous => (
                env_u64("RATE_LIMIT_ANON_RPM", 30),
                env_u64("RATE_LIMIT_ANON_BURST", 10),
            ),
            CallerTier::Free => (
                env_u64("RATE_LIMIT_FREE_RPM", 120),
                env_u64("RATE_LIMIT_FREE_BURST", 30),
            ),
            CallerTier::Premium => (
                env_u64("RATE_LIMIT_PREMIUM_RPM", 600),
                env_u64("RATE_LIMIT_PREMIUM_BURST", 120),
            ),
            CallerTier::Admin => (
                env_u64("RATE_LIMIT_ADMIN_RPM", 6000),
                env_u64("RATE_LIMIT_ADMIN_BURST", 1000),
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CallerTier::Anonymous => "anonymous",
            CallerTier::Free => "free",
            CallerTier::Premium => "premium",
            CallerTier::Admin => "admin",
        }
    }
}

// ── Route-level overrides ─────────────────────────────────────────────────────

/// Per-route rate-limit configuration.  Values are loaded from environment
/// variables with path-derived names, e.g. for `/api/v1/tips`:
/// - `ROUTE_RL_TIPS_RPM`
/// - `ROUTE_RL_TIPS_BURST`
#[derive(Debug, Clone)]
pub struct RouteRateConfig {
    pub rpm: u64,
    pub burst: u64,
}

/// Return a route-specific override if one is configured via env vars.
/// The path is normalised to upper-snake-case, e.g. `/api/v1/tips` → `TIPS`.
fn route_override(path: &str) -> Option<RouteRateConfig> {
    // Extract the last meaningful path segment as the key.
    let segment = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('v'))
        .last()?;

    let key = segment.to_uppercase().replace('-', "_");
    let rpm_var = format!("ROUTE_RL_{}_RPM", key);
    let burst_var = format!("ROUTE_RL_{}_BURST", key);

    let rpm = std::env::var(&rpm_var).ok().and_then(|v| v.parse().ok())?;
    let burst = std::env::var(&burst_var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(rpm / 4);

    Some(RouteRateConfig { rpm, burst })
}

// ── Identity helpers ──────────────────────────────────────────────────────────

/// Derive the caller tier from the injected `GatewayIdentity`.
fn tier_from_identity(identity: Option<&GatewayIdentity>) -> CallerTier {
    match identity {
        Some(GatewayIdentity::Jwt { role, .. }) => match role.as_str() {
            "admin" | "superadmin" => CallerTier::Admin,
            "premium" => CallerTier::Premium,
            _ => CallerTier::Free,
        },
        Some(GatewayIdentity::ApiKey { permissions, .. }) => {
            if permissions.iter().any(|p| p == "*" || p == "admin") {
                CallerTier::Admin
            } else if permissions.iter().any(|p| p == "premium") {
                CallerTier::Premium
            } else {
                CallerTier::Free
            }
        }
        Some(GatewayIdentity::Anonymous) | None => CallerTier::Anonymous,
    }
}

/// Build a stable client key used as the Redis counter key.
///
/// Priority: authenticated identity → IP address → constant fallback.
fn client_key(identity: Option<&GatewayIdentity>, req: &Request) -> String {
    if let Some(id) = identity {
        match id {
            GatewayIdentity::Jwt { subject, .. } => return format!("rl:jwt:{}", subject),
            GatewayIdentity::ApiKey { key, .. } => {
                return format!("rl:apikey:{}", &key[..key.len().min(16)])
            }
            GatewayIdentity::Anonymous => {}
        }
    }

    // Fall back to IP.
    let ip = req
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!("rl:ip:{}", ip)
}

// ── Redis sliding-window counter ──────────────────────────────────────────────

/// Increment a sliding-window counter stored in Redis and return the current
/// count.  Returns `None` on Redis error (fail-open strategy).
///
/// Window buckets are 60-second aligned.  Two buckets are used to implement a
/// sliding window: the current bucket + the previous bucket proportionally.
async fn redis_sliding_count(
    conn: &mut redis::aio::ConnectionManager,
    key: &str,
    window_secs: u64,
    now_ts: u64,
) -> Option<f64> {
    let bucket = now_ts / window_secs;
    let cur_key = format!("{}:{}", key, bucket);
    let prev_key = format!("{}:{}", key, bucket.saturating_sub(1));

    // INCR current bucket and get both buckets atomically via pipeline.
    let (cur_count, prev_count): (i64, i64) = redis::pipe()
        .atomic()
        .incr(&cur_key, 1i64)
        .expire(&cur_key, (window_secs * 2) as i64)
        .ignore()
        .get(&prev_key)
        .query_async(conn)
        .await
        .ok()?;

    let prev_count = prev_count.max(0);

    // Fraction of the current window that has elapsed.
    let elapsed_fraction = (now_ts % window_secs) as f64 / window_secs as f64;

    // Sliding window estimate = previous × (1 − elapsed) + current.
    let sliding = (prev_count as f64) * (1.0 - elapsed_fraction) + cur_count as f64;
    Some(sliding)
}

/// Increment a burst counter with a short TTL (10 s) and return the count.
async fn redis_burst_count(
    conn: &mut redis::aio::ConnectionManager,
    key: &str,
) -> Option<i64> {
    redis::pipe()
        .atomic()
        .incr(key, 1i64)
        .expire(key, 10i64)
        .ignore()
        .query_async::<(i64,)>(conn)
        .await
        .ok()
        .map(|(c,)| c)
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn add_rate_limit_headers(resp: &mut Response, limit: u64, remaining: i64, reset_secs: u64) {
    let headers = resp.headers_mut();
    let _ = headers.insert(
        "X-RateLimit-Limit",
        HeaderValue::from_str(&limit.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    let _ = headers.insert(
        "X-RateLimit-Remaining",
        HeaderValue::from_str(&remaining.max(0).to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );
    let _ = headers.insert(
        "X-RateLimit-Reset",
        HeaderValue::from_str(&reset_secs.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
}

fn too_many_requests(
    message: &'static str,
    retry_after: u64,
    tier: &str,
    limit: u64,
    reset_secs: u64,
) -> Response {
    let body = serde_json::json!({
        "error": message,
        "code": "RATE_LIMIT_EXCEEDED",
        "status": StatusCode::TOO_MANY_REQUESTS.as_u16(),
        "details": {
            "tier": tier,
            "limit_rpm": limit,
            "retry_after_secs": retry_after,
            "reset_at_secs": reset_secs,
        },
        "request_id": crate::middleware::request_id::current_request_id(),
    });

    let mut resp = (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
    if let Ok(v) = retry_after.to_string().parse() {
        resp.headers_mut().insert("Retry-After", v);
    }
    add_rate_limit_headers(&mut resp, limit, 0, reset_secs);
    resp
}

// ── Axum middleware ───────────────────────────────────────────────────────────

/// Gateway rate-limiting middleware.
///
/// - Resolves the caller tier from `GatewayIdentity` (injected by
///   `gateway_auth`).
/// - Applies per-route overrides when configured via env vars.
/// - Uses Redis sliding-window + burst counters when Redis is available.
/// - Falls back to allowing the request when Redis is unreachable
///   (tower_governor IP-based layers still apply as a backstop).
/// - Injects `X-RateLimit-*` headers on every response.
/// - Emits Prometheus metrics for observability.
pub async fn gateway_rate_limit(
    State(state): State<std::sync::Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_owned();
    let identity = req.extensions().get::<GatewayIdentity>().cloned();
    let tier = tier_from_identity(identity.as_ref());
    let tier_str = tier.as_str();

    // Effective limits: route override wins over tier default.
    let (rpm, burst) = if let Some(ov) = route_override(&path) {
        (ov.rpm, ov.burst)
    } else {
        tier.limits()
    };

    let key = client_key(identity.as_ref(), &req);

    RATE_LIMIT_REQUESTS_TOTAL
        .with_label_values(&[tier_str, &path])
        .inc();

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let window_secs = 60u64;
    let reset_secs = window_secs - (now_ts % window_secs);

    // ── Redis path ────────────────────────────────────────────────────────────
    if let Some(ref conn_mgr) = state.redis {
        let mut conn = conn_mgr.clone();

        // 1. Burst check (10-second sub-window).
        let burst_key = format!("{}:burst:{}", key, now_ts / 10);
        match redis_burst_count(&mut conn, &burst_key).await {
            Some(burst_count) if burst_count > burst as i64 => {
                RATE_LIMIT_EXCEEDED_TOTAL
                    .with_label_values(&[tier_str, "burst", &path])
                    .inc();
                RATE_LIMIT_BURST_CONSUMED_TOTAL
                    .with_label_values(&[tier_str])
                    .inc();
                tracing::warn!(
                    tier = tier_str,
                    key = %key,
                    burst_count,
                    burst_limit = burst,
                    path = %path,
                    "Burst rate limit exceeded"
                );
                return too_many_requests(
                    "Burst limit exceeded. Please slow down.",
                    10 - (now_ts % 10),
                    tier_str,
                    burst,
                    reset_secs,
                );
            }
            Some(_) => {}
            None => {
                tracing::debug!("Redis burst check failed – skipping burst enforcement");
            }
        }

        // 2. Sustained sliding-window check.
        let sliding_key = format!("{}:rpm", key);
        match redis_sliding_count(&mut conn, &sliding_key, window_secs, now_ts).await {
            Some(count) if count > rpm as f64 => {
                RATE_LIMIT_EXCEEDED_TOTAL
                    .with_label_values(&[tier_str, "sustained", &path])
                    .inc();
                tracing::warn!(
                    tier = tier_str,
                    key = %key,
                    count,
                    rpm,
                    path = %path,
                    "Sustained rate limit exceeded"
                );
                return too_many_requests(
                    "Rate limit exceeded. Please slow down.",
                    reset_secs,
                    tier_str,
                    rpm,
                    reset_secs,
                );
            }
            Some(count) => {
                // Attach rate-limit info to extensions for quota manager.
                let remaining = (rpm as f64 - count).max(0.0) as i64;
                let mut resp = next.run(req).await;
                add_rate_limit_headers(&mut resp, rpm, remaining, reset_secs);
                resp.headers_mut().insert(
                    "X-RateLimit-Tier",
                    HeaderValue::from_static(tier_str),
                );
                return resp;
            }
            None => {
                tracing::debug!("Redis sliding-window check failed – falling through");
            }
        }
    }

    // ── No Redis / fail-open path ─────────────────────────────────────────────
    let mut resp = next.run(req).await;
    add_rate_limit_headers(&mut resp, rpm, rpm as i64, reset_secs);
    resp.headers_mut().insert(
        "X-RateLimit-Tier",
        HeaderValue::from_static(tier_str),
    );
    resp
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_limits_respect_defaults() {
        let (rpm, burst) = CallerTier::Anonymous.limits();
        assert!(rpm > 0);
        assert!(burst > 0);
        assert!(burst < rpm);

        let (free_rpm, _) = CallerTier::Free.limits();
        let (premium_rpm, _) = CallerTier::Premium.limits();
        assert!(free_rpm < premium_rpm);
    }

    #[test]
    fn client_key_for_anonymous_falls_back_gracefully() {
        // Build a minimal request with no extensions.
        use axum::body::Body;
        let req = axum::http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let key = client_key(None, &req);
        assert!(key.starts_with("rl:ip:"));
    }

    #[test]
    fn route_override_returns_none_for_unknown_segment() {
        assert!(route_override("/api/v1/completely_unknown_xyz").is_none());
    }
}
