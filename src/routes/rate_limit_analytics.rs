
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    db::connection::AppState,
    errors::AppError,
    gateway::{
        analytics as rl_analytics,
        quota_manager::{get_daily_quotas, upsert_client_quota, QuotaPeriod},
    },
    middleware::admin_auth::require_admin,
};

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TopOffendersQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Deserialize)]
pub struct PathBreakdownQuery {
    #[serde(default = "default_path_limit")]
    pub limit: i64,
}

fn default_path_limit() -> i64 {
    30
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpsertQuotaRequest {
    /// Maximum requests allowed in the period.
    pub max_requests: i64,
    /// `"daily"` or `"monthly"`.  Defaults to daily.
    #[serde(default = "default_period")]
    pub period: String,
}

fn default_period() -> String {
    "daily".to_string()
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        // ── Analytics ──────────────────────────────────────────────────────
        .route(
            "/admin/rate-limits/analytics",
            get(full_analytics_summary),
        )
        .route(
            "/admin/rate-limits/analytics/timeseries",
            get(timeseries),
        )
        .route(
            "/admin/rate-limits/analytics/top-offenders",
            get(top_offenders),
        )
        .route(
            "/admin/rate-limits/analytics/tiers",
            get(tier_breakdown),
        )
        .route(
            "/admin/rate-limits/analytics/paths",
            get(path_breakdown),
        )
        // ── Quota management ───────────────────────────────────────────────
        .route("/admin/rate-limits/quotas", get(list_quotas))
        .route(
            "/admin/rate-limits/quotas/:client_id",
            put(upsert_quota),
        )
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Full analytics summary: totals, top offenders, tier & path breakdowns,
/// and 24-hour time series.
async fn full_analytics_summary(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let summary = rl_analytics::get_summary(&state.db).await?;
    Ok((StatusCode::OK, Json(summary)))
}

/// Hourly blocked-request time series for the last 24 hours.
async fn timeseries(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let series = rl_analytics::time_series_last_24h(&state.db).await?;
    Ok((StatusCode::OK, Json(series)))
}

/// Top blocked client IDs in the last 24 hours.
async fn top_offenders(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TopOffendersQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.clamp(1, 100);
    let offenders = rl_analytics::top_offenders(&state.db, limit).await?;
    Ok((StatusCode::OK, Json(offenders)))
}

/// Per-tier (anonymous / free / premium / admin) breakdown of blocked requests.
async fn tier_breakdown(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let rows = rl_analytics::tier_breakdown(&state.db).await?;
    Ok((StatusCode::OK, Json(rows)))
}

/// Per-path breakdown: most-blocked endpoints.
async fn path_breakdown(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathBreakdownQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.clamp(1, 100);
    let rows = rl_analytics::path_breakdown(&state.db, limit).await?;
    Ok((StatusCode::OK, Json(rows)))
}

/// List all daily quota rows for the current period (up to 500 clients).
async fn list_quotas(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let quotas = get_daily_quotas(&state.db).await?;
    Ok((StatusCode::OK, Json(quotas)))
}

/// Create or update the quota limit for a specific client.
///
/// `PUT /admin/rate-limits/quotas/:client_id`
/// Body: `{ "max_requests": 50000, "period": "daily" }`
async fn upsert_quota(
    State(state): State<Arc<AppState>>,
    Path(client_id): Path<String>,
    Json(body): Json<UpsertQuotaRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.max_requests <= 0 {
        return Err(AppError::bad_request(
            "max_requests must be a positive integer",
        ));
    }

    let period = match body.period.as_str() {
        "monthly" => QuotaPeriod::Monthly,
        _ => QuotaPeriod::Daily,
    };

    let quota = upsert_client_quota(&state.db, &client_id, period, body.max_requests).await?;
    Ok((StatusCode::OK, Json(quota)))
}
