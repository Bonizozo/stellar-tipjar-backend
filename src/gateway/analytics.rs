
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::errors::AppError;

// ── Event types ───────────────────────────────────────────────────────────────

/// The kind of limit that was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// Short-window burst limit.
    Burst,
    /// Sustained requests-per-minute limit.
    Sustained,
    /// Daily/monthly quota exhausted.
    Quota,
}

impl LimitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LimitKind::Burst => "burst",
            LimitKind::Sustained => "sustained",
            LimitKind::Quota => "quota",
        }
    }
}

// ── Write path ────────────────────────────────────────────────────────────────

/// Record a rate-limit rejection event asynchronously.
///
/// The call is fire-and-forget; failures are logged as warnings but do not
/// affect the calling middleware.
pub fn record_event(
    db: Arc<PgPool>,
    client_id: String,
    tier: String,
    path: String,
    kind: LimitKind,
    limit_value: i64,
    request_count: i64,
) {
    tokio::spawn(async move {
        let kind_str = kind.as_str();
        if let Err(e) = sqlx::query(
            r#"
            INSERT INTO rate_limit_events
                (client_id, tier, path, kind, limit_value, request_count, occurred_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            "#,
        )
        .bind(&client_id)
        .bind(&tier)
        .bind(&path)
        .bind(kind_str)
        .bind(limit_value)
        .bind(request_count)
        .execute(&*db)
        .await
        {
            tracing::warn!(
                error = %e,
                client_id = %client_id,
                kind = kind_str,
                "Failed to record rate limit event"
            );
        }
    });
}

// ── Query types ───────────────────────────────────────────────────────────────

/// Time-bucketed count of blocked requests.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BlockedTimeSeries {
    /// Truncated to the hour.
    pub bucket: DateTime<Utc>,
    pub count: i64,
    pub kind: String,
}

/// Top client IDs by blocked request count.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TopOffender {
    pub client_id: String,
    pub tier: String,
    pub blocked_count: i64,
    pub last_blocked_at: DateTime<Utc>,
}

/// Per-tier blocked request breakdown.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TierBreakdown {
    pub tier: String,
    pub kind: String,
    pub blocked_count: i64,
}

/// Path-level blocked request summary.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PathBreakdown {
    pub path: String,
    pub blocked_count: i64,
    pub unique_clients: i64,
}

/// Aggregated analytics summary for the admin dashboard.
#[derive(Debug, Serialize)]
pub struct RateLimitAnalyticsSummary {
    pub total_blocked_last_hour: i64,
    pub total_blocked_last_24h: i64,
    pub top_offenders: Vec<TopOffender>,
    pub tier_breakdown: Vec<TierBreakdown>,
    pub path_breakdown: Vec<PathBreakdown>,
    pub time_series_last_24h: Vec<BlockedTimeSeries>,
}

// ── Query helpers ─────────────────────────────────────────────────────────────

pub async fn blocked_last_hour(db: &PgPool) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM rate_limit_events WHERE occurred_at > NOW() - INTERVAL '1 hour'",
    )
    .fetch_one(db)
    .await?;
    Ok(row.0)
}

pub async fn blocked_last_24h(db: &PgPool) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM rate_limit_events WHERE occurred_at > NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(db)
    .await?;
    Ok(row.0)
}

pub async fn top_offenders(db: &PgPool, limit: i64) -> Result<Vec<TopOffender>, AppError> {
    let rows = sqlx::query_as::<_, TopOffender>(
        r#"
        SELECT
            client_id,
            tier,
            COUNT(*)           AS blocked_count,
            MAX(occurred_at)   AS last_blocked_at
        FROM   rate_limit_events
        WHERE  occurred_at > NOW() - INTERVAL '24 hours'
        GROUP  BY client_id, tier
        ORDER  BY blocked_count DESC
        LIMIT  $1
        "#,
    )
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn tier_breakdown(db: &PgPool) -> Result<Vec<TierBreakdown>, AppError> {
    let rows = sqlx::query_as::<_, TierBreakdown>(
        r#"
        SELECT
            tier,
            kind,
            COUNT(*) AS blocked_count
        FROM   rate_limit_events
        WHERE  occurred_at > NOW() - INTERVAL '24 hours'
        GROUP  BY tier, kind
        ORDER  BY blocked_count DESC
        "#,
    )
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn path_breakdown(db: &PgPool, limit: i64) -> Result<Vec<PathBreakdown>, AppError> {
    let rows = sqlx::query_as::<_, PathBreakdown>(
        r#"
        SELECT
            path,
            COUNT(*)                    AS blocked_count,
            COUNT(DISTINCT client_id)   AS unique_clients
        FROM   rate_limit_events
        WHERE  occurred_at > NOW() - INTERVAL '24 hours'
        GROUP  BY path
        ORDER  BY blocked_count DESC
        LIMIT  $1
        "#,
    )
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn time_series_last_24h(db: &PgPool) -> Result<Vec<BlockedTimeSeries>, AppError> {
    let rows = sqlx::query_as::<_, BlockedTimeSeries>(
        r#"
        SELECT
            DATE_TRUNC('hour', occurred_at) AS bucket,
            kind,
            COUNT(*)                        AS count
        FROM   rate_limit_events
        WHERE  occurred_at > NOW() - INTERVAL '24 hours'
        GROUP  BY DATE_TRUNC('hour', occurred_at), kind
        ORDER  BY bucket ASC
        "#,
    )
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// Build a full analytics summary — used by the admin route.
pub async fn get_summary(db: &PgPool) -> Result<RateLimitAnalyticsSummary, AppError> {
    let (total_1h, total_24h, top, tiers, paths, series) = tokio::try_join!(
        blocked_last_hour(db),
        blocked_last_24h(db),
        top_offenders(db, 20),
        tier_breakdown(db),
        path_breakdown(db, 30),
        time_series_last_24h(db),
    )?;

    Ok(RateLimitAnalyticsSummary {
        total_blocked_last_hour: total_1h,
        total_blocked_last_24h: total_24h,
        top_offenders: top,
        tier_breakdown: tiers,
        path_breakdown: paths,
        time_series_last_24h: series,
    })
}
