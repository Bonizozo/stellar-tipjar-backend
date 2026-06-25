use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    db::connection::AppState,
    errors::AppError,
    gateway::context::GatewayIdentity,
    metrics::collectors::{QUOTA_EXCEEDED_TOTAL, QUOTA_USAGE_RATIO},
};

// ── Quota period ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "lowercase")]
pub enum QuotaPeriod {
    Daily,
    Monthly,
}

impl QuotaPeriod {
    /// ISO-8601 date string representing the start of the current period.
    pub fn current_period_start(self) -> String {
        let now = Utc::now();
        match self {
            QuotaPeriod::Daily => now.format("%Y-%m-%d").to_string(),
            QuotaPeriod::Monthly => format!("{}-{:02}-01", now.year(), now.month()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            QuotaPeriod::Daily => "daily",
            QuotaPeriod::Monthly => "monthly",
        }
    }
}

// ── Data model ────────────────────────────────────────────────────────────────

/// A row from `api_client_quotas`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClientQuota {
    pub client_id: String,
    pub period: String,
    pub max_requests: i64,
    pub used_requests: i64,
    pub period_start: String,
    pub enabled: bool,
}

impl ClientQuota {
    pub fn remaining(&self) -> i64 {
        (self.max_requests - self.used_requests).max(0)
    }

    pub fn is_exhausted(&self) -> bool {
        self.used_requests >= self.max_requests
    }

    pub fn usage_ratio(&self) -> f64 {
        if self.max_requests == 0 {
            1.0
        } else {
            self.used_requests as f64 / self.max_requests as f64
        }
    }
}

// ── Tier defaults ─────────────────────────────────────────────────────────────

fn tier_daily_quota(tier: &str) -> i64 {
    match tier {
        "anonymous" => env_i64("QUOTA_ANON_DAILY", 1_000),
        "free" => env_i64("QUOTA_FREE_DAILY", 10_000),
        "premium" => env_i64("QUOTA_PREMIUM_DAILY", 100_000),
        "admin" => env_i64("QUOTA_ADMIN_DAILY", 1_000_000),
        _ => 5_000,
    }
}

fn tier_monthly_quota(tier: &str) -> i64 {
    match tier {
        "anonymous" => env_i64("QUOTA_ANON_MONTHLY", 10_000),
        "free" => env_i64("QUOTA_FREE_MONTHLY", 100_000),
        "premium" => env_i64("QUOTA_PREMIUM_MONTHLY", 1_000_000),
        "admin" => env_i64("QUOTA_ADMIN_MONTHLY", 10_000_000),
        _ => 50_000,
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────────

/// Fetch the quota row for `client_id` in the current period.
/// Returns `None` when no custom quota row exists.
async fn fetch_quota(
    db: &PgPool,
    client_id: &str,
    period: QuotaPeriod,
) -> Option<ClientQuota> {
    let period_start = period.current_period_start();
    sqlx::query_as::<_, ClientQuota>(
        r#"
        SELECT client_id, period, max_requests, used_requests, period_start, enabled
        FROM   api_client_quotas
        WHERE  client_id   = $1
          AND  period      = $2
          AND  period_start = $3
          AND  enabled     = true
        "#,
    )
    .bind(client_id)
    .bind(period.as_str())
    .bind(&period_start)
    .fetch_optional(db)
    .await
    .ok()?
}

/// Atomically increment the usage counter.  Uses INSERT … ON CONFLICT so the
/// row is created (with tier-default max) on first use.
async fn increment_quota(
    db: &PgPool,
    client_id: &str,
    period: QuotaPeriod,
    tier: &str,
) -> Result<ClientQuota, sqlx::Error> {
    let period_start = period.current_period_start();
    let max_requests = match period {
        QuotaPeriod::Daily => tier_daily_quota(tier),
        QuotaPeriod::Monthly => tier_monthly_quota(tier),
    };

    sqlx::query_as::<_, ClientQuota>(
        r#"
        INSERT INTO api_client_quotas
            (client_id, period, period_start, max_requests, used_requests, enabled)
        VALUES ($1, $2, $3, $4, 1, true)
        ON CONFLICT (client_id, period, period_start)
        DO UPDATE SET used_requests = api_client_quotas.used_requests + 1
        RETURNING client_id, period, max_requests, used_requests, period_start, enabled
        "#,
    )
    .bind(client_id)
    .bind(period.as_str())
    .bind(&period_start)
    .bind(max_requests)
    .fetch_one(db)
    .await
}

// ── Client-ID derivation ──────────────────────────────────────────────────────

fn client_id_from_identity(identity: Option<&GatewayIdentity>, req: &Request) -> String {
    match identity {
        Some(GatewayIdentity::Jwt { subject, .. }) => format!("jwt:{}", subject),
        Some(GatewayIdentity::ApiKey { key, .. }) => format!("apikey:{}", &key[..key.len().min(16)]),
        _ => {
            use axum::extract::ConnectInfo;
            req.extensions()
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| format!("ip:{}", ci.0.ip()))
                .unwrap_or_else(|| "ip:unknown".to_string())
        }
    }
}

fn tier_from_identity(identity: Option<&GatewayIdentity>) -> &'static str {
    match identity {
        Some(GatewayIdentity::Jwt { role, .. }) => match role.as_str() {
            "admin" | "superadmin" => "admin",
            "premium" => "premium",
            _ => "free",
        },
        Some(GatewayIdentity::ApiKey { permissions, .. }) => {
            if permissions.iter().any(|p| p == "*" || p == "admin") {
                "admin"
            } else if permissions.iter().any(|p| p == "premium") {
                "premium"
            } else {
                "free"
            }
        }
        Some(GatewayIdentity::Anonymous) | None => "anonymous",
    }
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn quota_too_many_requests(
    client_id: &str,
    period: QuotaPeriod,
    used: i64,
    max: i64,
) -> Response {
    let reset_hint = match period {
        QuotaPeriod::Daily => "at midnight UTC",
        QuotaPeriod::Monthly => "at the start of next month",
    };
    let body = serde_json::json!({
        "error": "API quota exhausted",
        "code": "QUOTA_EXCEEDED",
        "status": StatusCode::TOO_MANY_REQUESTS.as_u16(),
        "details": {
            "client_id": client_id,
            "period": period.as_str(),
            "used": used,
            "limit": max,
            "resets": reset_hint,
        },
        "request_id": crate::middleware::request_id::current_request_id(),
    });
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
    resp.headers_mut().insert(
        "X-Quota-Limit",
        HeaderValue::from_str(&max.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    resp.headers_mut().insert(
        "X-Quota-Remaining",
        HeaderValue::from_static("0"),
    );
    resp.headers_mut().insert(
        "X-Quota-Period",
        HeaderValue::from_static(period.as_str()),
    );
    resp
}

fn inject_quota_headers(resp: &mut Response, quota: &ClientQuota) {
    let headers = resp.headers_mut();
    let _ = headers.insert(
        "X-Quota-Limit",
        HeaderValue::from_str(&quota.max_requests.to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );
    let _ = headers.insert(
        "X-Quota-Remaining",
        HeaderValue::from_str(&quota.remaining().to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );
    let _ = headers.insert(
        "X-Quota-Period",
        HeaderValue::from_static(quota.period.as_str()),
    );
}

// ── Middleware ────────────────────────────────────────────────────────────────

/// Axum middleware that enforces per-client API quotas.
///
/// Quota enforcement is opt-in: if the feature is disabled via the
/// `QUOTA_ENFORCEMENT_ENABLED` env var (default `true`), all requests pass
/// through with quota headers still populated.
pub async fn quota_enforcement(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let enabled = std::env::var("QUOTA_ENFORCEMENT_ENABLED")
        .map(|v| v.to_lowercase() != "false")
        .unwrap_or(true);

    if !enabled {
        return next.run(req).await;
    }

    let identity = req.extensions().get::<GatewayIdentity>().cloned();
    let client_id = client_id_from_identity(identity.as_ref(), &req);
    let tier = tier_from_identity(identity.as_ref());

    // Use daily quotas by default; monthly quotas can be layered in separately.
    let period = QuotaPeriod::Daily;

    // ── Check existing quota before incrementing ──────────────────────────────
    if let Some(existing) = fetch_quota(&state.db, &client_id, period).await {
        if existing.is_exhausted() {
            QUOTA_EXCEEDED_TOTAL.with_label_values(&[tier, period.as_str()]).inc();
            tracing::warn!(
                client_id = %client_id,
                tier,
                used = existing.used_requests,
                max = existing.max_requests,
                "Quota exhausted"
            );
            return quota_too_many_requests(
                &client_id,
                period,
                existing.used_requests,
                existing.max_requests,
            );
        }
    }

    // ── Increment quota asynchronously post-response ──────────────────────────
    let db = state.db.clone();
    let cid = client_id.clone();
    let tier_owned = tier.to_owned();

    let mut resp = next.run(req).await;

    // Fire-and-forget increment (non-blocking).
    tokio::spawn(async move {
        match increment_quota(&db, &cid, period, &tier_owned).await {
            Ok(quota) => {
                let ratio = quota.usage_ratio();
                QUOTA_USAGE_RATIO
                    .with_label_values(&[tier_owned.as_str(), period.as_str()])
                    .set(ratio);

                if ratio >= 0.9 {
                    tracing::warn!(
                        client_id = %cid,
                        tier = %tier_owned,
                        used = quota.used_requests,
                        max = quota.max_requests,
                        ratio,
                        "Quota nearing exhaustion (≥90%)"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, client_id = %cid, "Failed to increment quota counter");
            }
        }
    });

    // Inject quota headers using tier-default values (since we didn't wait for the DB).
    let max_daily = tier_daily_quota(tier);
    let dummy_quota = ClientQuota {
        client_id: client_id.clone(),
        period: period.as_str().to_string(),
        max_requests: max_daily,
        used_requests: 0, // approximate – exact value is from DB post-increment
        period_start: period.current_period_start(),
        enabled: true,
    };
    inject_quota_headers(&mut resp, &dummy_quota);

    resp
}

// ── Admin helpers (used by the analytics route) ───────────────────────────────

/// Summary row returned by the quota analytics endpoint.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct QuotaSummary {
    pub client_id: String,
    pub period: String,
    pub period_start: String,
    pub max_requests: i64,
    pub used_requests: i64,
    pub enabled: bool,
}

/// Fetch all quota rows for the current daily period.
pub async fn get_daily_quotas(db: &PgPool) -> Result<Vec<QuotaSummary>, AppError> {
    let period_start = QuotaPeriod::Daily.current_period_start();
    let rows = sqlx::query_as::<_, QuotaSummary>(
        r#"
        SELECT client_id, period, period_start, max_requests, used_requests, enabled
        FROM   api_client_quotas
        WHERE  period       = 'daily'
          AND  period_start = $1
        ORDER  BY used_requests DESC
        LIMIT  500
        "#,
    )
    .bind(&period_start)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// Fetch or create a quota override for a specific client.
pub async fn upsert_client_quota(
    db: &PgPool,
    client_id: &str,
    period: QuotaPeriod,
    max_requests: i64,
) -> Result<QuotaSummary, AppError> {
    let period_start = period.current_period_start();
    let row = sqlx::query_as::<_, QuotaSummary>(
        r#"
        INSERT INTO api_client_quotas
            (client_id, period, period_start, max_requests, used_requests, enabled)
        VALUES ($1, $2, $3, $4, 0, true)
        ON CONFLICT (client_id, period, period_start)
        DO UPDATE SET max_requests = EXCLUDED.max_requests
        RETURNING client_id, period, period_start, max_requests, used_requests, enabled
        "#,
    )
    .bind(client_id)
    .bind(period.as_str())
    .bind(&period_start)
    .bind(max_requests)
    .fetch_one(db)
    .await?;
    Ok(row)
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
