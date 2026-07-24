use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use lazy_static::lazy_static;

use crate::{
    db::connection::AppState,
    gateway::client_ip::extract_client_ip,
    gateway::context::GatewayIdentity,
    gateway::rate_limit_policy::{self, FailPolicy},
    gateway::rate_limit_script::GcraLimiter,
    metrics::collectors::{
        RATE_LIMIT_DEGRADED_TOTAL, RATE_LIMIT_EXCEEDED_TOTAL, RATE_LIMIT_REQUESTS_TOTAL,
    },
};

lazy_static! {
    /// Single compiled GCRA script, shared across all requests so the SHA
    /// cache inside `redis::Script` is reused instead of being rebuilt per
    /// request.
    static ref GCRA: GcraLimiter = GcraLimiter::new();
}

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
/// Priority: authenticated identity → trusted client IP → constant fallback.
/// The IP fallback goes through [`extract_client_ip`], which only trusts
/// `X-Forwarded-For` up to the configured `TRUSTED_PROXY_DEPTH` — a naive
/// `ConnectInfo`-only or header-only lookup would let a client behind our own
/// load balancer either collide with (or spoof around) another client's
/// bucket.
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

    format!("rl:ip:{}", extract_client_ip(req))
}

// ── Response helpers ──────────────────────────────────────────────────────────

/// Emit both the legacy `X-RateLimit-*` headers (kept for existing clients)
/// and the standard IETF `RateLimit-*` headers
/// (draft-ietf-httpapi-ratelimit-headers) that new clients should prefer.
fn add_rate_limit_headers(resp: &mut Response, limit: u64, remaining: i64, reset_secs: u64) {
    let headers = resp.headers_mut();
    let limit_val =
        HeaderValue::from_str(&limit.to_string()).unwrap_or(HeaderValue::from_static("0"));
    let remaining_val = HeaderValue::from_str(&remaining.max(0).to_string())
        .unwrap_or(HeaderValue::from_static("0"));
    let reset_val =
        HeaderValue::from_str(&reset_secs.to_string()).unwrap_or(HeaderValue::from_static("0"));

    let _ = headers.insert("X-RateLimit-Limit", limit_val.clone());
    let _ = headers.insert("X-RateLimit-Remaining", remaining_val.clone());
    let _ = headers.insert("X-RateLimit-Reset", reset_val.clone());

    let _ = headers.insert("RateLimit-Limit", limit_val);
    let _ = headers.insert("RateLimit-Remaining", remaining_val);
    let _ = headers.insert("RateLimit-Reset", reset_val);
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

/// Response returned when the limiter is degraded (Redis unreachable) and the
/// resolved [`FailPolicy`] for this route is `FailClosed`. Deliberately a
/// `503`, not a `429` — we cannot verify the caller is actually over their
/// limit, so this communicates "temporarily can't serve you safely" rather
/// than "you are rate limited".
fn degraded_fail_closed(tier: &str) -> Response {
    let body = serde_json::json!({
        "error": "Rate limiting is temporarily unavailable and this endpoint fails closed for safety. Please retry shortly.",
        "code": "RATE_LIMIT_DEGRADED",
        "status": StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        "details": { "tier": tier },
        "request_id": crate::middleware::request_id::current_request_id(),
    });
    let mut resp = (StatusCode::SERVICE_UNAVAILABLE, axum::Json(body)).into_response();
    resp.headers_mut()
        .insert("Retry-After", HeaderValue::from_static("1"));
    resp
}

// ── Axum middleware ───────────────────────────────────────────────────────────

/// Gateway rate-limiting middleware.
///
/// - Resolves the caller tier from `GatewayIdentity` (injected by
///   `gateway_auth`).
/// - Applies per-route overrides when configured via env vars.
/// - Enforces a single atomic GCRA check per request via a Lua script in
///   Redis (see `rate_limit_script`), shared across every replica — burst and
///   sustained rate are both captured by the one call, with no
///   read-modify-write race.
/// - When Redis is unreachable, consults the per-route [`FailPolicy`]
///   (`rate_limit_policy`): fail-closed routes (auth/account-security) reject
///   with `503`; fail-open routes fall through, with local `tower_governor`
///   IP-based layers acting as a backstop.
/// - Injects both legacy `X-RateLimit-*` and standard `RateLimit-*` headers,
///   plus `Retry-After` on `429`s.
/// - Emits Prometheus metrics for observability, including degraded-mode
///   decisions.
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

    let window_secs = 60u64;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // ── Redis path: single atomic GCRA check ─────────────────────────────────
    // `state.redis` being `None` (never configured, or the initial connection
    // failed at boot) is just as much a "Redis is unavailable" condition as a
    // per-request script error — both must go through the same fail-policy
    // decision below, rather than letting an absent connection silently
    // bypass fail-closed routes.
    let gcra_result = match &state.redis {
        Some(conn_mgr) => {
            let mut conn = conn_mgr.clone();
            let gcra_key = format!("{}:gcra", key);
            Some(
                GCRA.check(&mut conn, &gcra_key, rpm, burst, window_secs, now_ms)
                    .await,
            )
        }
        None => None,
    };

    match gcra_result {
        Some(Ok(decision)) if !decision.allowed => {
            RATE_LIMIT_EXCEEDED_TOTAL
                .with_label_values(&[tier_str, "gcra", &path])
                .inc();
            tracing::warn!(
                tier = tier_str,
                key = %key,
                rpm,
                burst,
                path = %path,
                "Rate limit exceeded"
            );
            return too_many_requests(
                "Rate limit exceeded. Please slow down.",
                decision.retry_after_secs,
                tier_str,
                decision.limit as u64,
                decision.reset_after_secs,
            );
        }
        Some(Ok(decision)) => {
            let mut resp = next.run(req).await;
            add_rate_limit_headers(
                &mut resp,
                decision.limit as u64,
                decision.remaining,
                decision.reset_after_secs,
            );
            resp.headers_mut()
                .insert("X-RateLimit-Tier", HeaderValue::from_static(tier_str));
            return resp;
        }
        Some(Err(e)) => {
            tracing::error!(
                error = %e,
                tier = tier_str,
                path = %path,
                "Rate limiter Redis check failed; applying degraded-mode fail policy"
            );
        }
        None => {
            tracing::debug!(
                tier = tier_str,
                path = %path,
                "Rate limiter has no Redis connection; applying degraded-mode fail policy"
            );
        }
    }

    // ── Degraded path (Redis absent or errored) ──────────────────────────────
    // Reached whenever Redis could not be consulted at all. The per-route
    // fail policy decides the outcome explicitly rather than defaulting
    // either way; `tower_governor`'s per-process IP layers remain a coarse
    // backstop on the fail-open branch.
    let policy = rate_limit_policy::resolve(&path);
    if policy == FailPolicy::FailClosed {
        RATE_LIMIT_DEGRADED_TOTAL
            .with_label_values(&[tier_str, policy.as_str(), "rejected"])
            .inc();
        return degraded_fail_closed(tier_str);
    }
    RATE_LIMIT_DEGRADED_TOTAL
        .with_label_values(&[tier_str, policy.as_str(), "allowed"])
        .inc();

    let reset_secs = window_secs - ((now_ms as u64 / 1000) % window_secs);
    let mut resp = next.run(req).await;
    add_rate_limit_headers(&mut resp, rpm, rpm as i64, reset_secs);
    resp.headers_mut()
        .insert("X-RateLimit-Tier", HeaderValue::from_static(tier_str));
    resp.headers_mut()
        .insert("X-RateLimit-Degraded", HeaderValue::from_static("true"));
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
