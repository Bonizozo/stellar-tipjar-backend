use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::cache::redis_client;
use crate::db::connection::AppState;
use crate::errors::AppResult;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct TimeSeriesPoint {
    pub period: String,
    pub tip_count: i64,
    pub total_amount: String,
    pub avg_amount: String,
    pub unique_creators: i64,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct PlatformSummary {
    pub total_creators: i64,
    pub total_tips: i64,
    pub total_amount: String,
    pub avg_tip_amount: String,
    pub tips_last_24h: i64,
    pub tips_last_7d: i64,
    pub tips_last_30d: i64,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreatorAnalytics {
    pub creator_username: String,
    pub tip_count: i64,
    pub total_amount: String,
    pub avg_amount: String,
    pub max_amount: String,
    pub first_tip_at: Option<DateTime<Utc>>,
    pub last_tip_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct AnalyticsQuery {
    /// Time range granularity: "day", "week", "month", "year"
    #[serde(default = "default_granularity")]
    pub granularity: String,
    /// Number of periods to return (default 30, max 365)
    #[serde(default = "default_periods")]
    pub periods: i64,
    /// Optional creator filter
    pub creator: Option<String>,
    /// Optional start date filter
    pub from: Option<DateTime<Utc>>,
    /// Optional end date filter
    pub to: Option<DateTime<Utc>>,
}

fn default_granularity() -> String {
    "day".to_string()
}

fn default_periods() -> i64 {
    30
}

impl AnalyticsQuery {
    pub fn clamped_periods(&self) -> i64 {
        self.periods.clamp(1, 365)
    }

    pub fn pg_trunc(&self) -> &str {
        match self.granularity.as_str() {
            "week" => "week",
            "month" => "month",
            "year" => "year",
            _ => "day",
        }
    }
}

const TTL_ANALYTICS: u64 = 300; // 5 minutes

pub async fn get_platform_summary(state: &AppState) -> AppResult<PlatformSummary> {
    const CACHE_KEY: &str = "analytics:platform:summary";

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        if let Some(cached) = redis_client::get::<PlatformSummary>(&mut conn, CACHE_KEY).await {
            return Ok(cached);
        }
    }

    let summary = sqlx::query_as::<_, PlatformSummary>(
        r#"
        SELECT
            (SELECT COUNT(*) FROM creators)::BIGINT AS total_creators,
            COUNT(*)::BIGINT AS total_tips,
            COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
            COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_tip_amount,
            COUNT(*) FILTER (WHERE created_at >= NOW() - INTERVAL '24 hours')::BIGINT AS tips_last_24h,
            COUNT(*) FILTER (WHERE created_at >= NOW() - INTERVAL '7 days')::BIGINT AS tips_last_7d,
            COUNT(*) FILTER (WHERE created_at >= NOW() - INTERVAL '30 days')::BIGINT AS tips_last_30d
        FROM tips
        "#,
    )
    .fetch_one(&state.db)
    .await?;

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        redis_client::set(&mut conn, CACHE_KEY, &summary, TTL_ANALYTICS).await;
    }

    Ok(summary)
}

pub async fn get_time_series(state: &AppState, query: &AnalyticsQuery) -> AppResult<Vec<TimeSeriesPoint>> {
    let periods = query.clamped_periods();
    let trunc = query.pg_trunc();
    let cache_key = format!(
        "analytics:timeseries:{}:{}:{}",
        trunc,
        periods,
        query.creator.as_deref().unwrap_or("all")
    );

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        if let Some(cached) = redis_client::get::<Vec<TimeSeriesPoint>>(&mut conn, &cache_key).await {
            return Ok(cached);
        }
    }

    let points = if let Some(ref creator) = query.creator {
        sqlx::query_as::<_, TimeSeriesPoint>(&format!(
            r#"
            SELECT
                DATE_TRUNC('{trunc}', created_at)::TEXT AS period,
                COUNT(*)::BIGINT AS tip_count,
                COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
                COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
                COUNT(DISTINCT creator_username)::BIGINT AS unique_creators
            FROM tips
            WHERE creator_username = $1
              AND created_at >= NOW() - ($2 || ' {trunc}s')::INTERVAL
            GROUP BY DATE_TRUNC('{trunc}', created_at)
            ORDER BY period DESC
            "#
        ))
        .bind(creator)
        .bind(periods)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, TimeSeriesPoint>(&format!(
            r#"
            SELECT
                DATE_TRUNC('{trunc}', created_at)::TEXT AS period,
                COUNT(*)::BIGINT AS tip_count,
                COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
                COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
                COUNT(DISTINCT creator_username)::BIGINT AS unique_creators
            FROM tips
            WHERE created_at >= NOW() - ($1 || ' {trunc}s')::INTERVAL
            GROUP BY DATE_TRUNC('{trunc}', created_at)
            ORDER BY period DESC
            "#
        ))
        .bind(periods)
        .fetch_all(&state.db)
        .await?
    };

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        redis_client::set(&mut conn, &cache_key, &points, TTL_ANALYTICS).await;
    }

    Ok(points)
}

pub async fn get_top_creators(state: &AppState, limit: i64) -> AppResult<Vec<CreatorAnalytics>> {
    let limit = limit.clamp(1, 100);
    let cache_key = format!("analytics:top_creators:{}", limit);

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        if let Some(cached) = redis_client::get::<Vec<CreatorAnalytics>>(&mut conn, &cache_key).await {
            return Ok(cached);
        }
    }

    let creators = sqlx::query_as::<_, CreatorAnalytics>(
        r#"
        SELECT
            creator_username,
            COUNT(*)::BIGINT AS tip_count,
            COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
            COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
            COALESCE(MAX(amount::NUMERIC), 0)::TEXT AS max_amount,
            MIN(created_at) AS first_tip_at,
            MAX(created_at) AS last_tip_at
        FROM tips
        GROUP BY creator_username
        ORDER BY SUM(amount::NUMERIC) DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        redis_client::set(&mut conn, &cache_key, &creators, TTL_ANALYTICS).await;
    }

    Ok(creators)
}

pub async fn get_creator_analytics(state: &AppState, username: &str) -> AppResult<CreatorAnalytics> {
    let cache_key = format!("analytics:creator:{}", username);

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        if let Some(cached) = redis_client::get::<CreatorAnalytics>(&mut conn, &cache_key).await {
            return Ok(cached);
        }
    }

    let analytics = sqlx::query_as::<_, CreatorAnalytics>(
        r#"
        SELECT
            $1::TEXT AS creator_username,
            COUNT(*)::BIGINT AS tip_count,
            COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
            COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
            COALESCE(MAX(amount::NUMERIC), 0)::TEXT AS max_amount,
            MIN(created_at) AS first_tip_at,
            MAX(created_at) AS last_tip_at
        FROM tips
        WHERE creator_username = $1
        "#,
    )
    .bind(username)
    .fetch_one(&state.db)
    .await?;

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        redis_client::set(&mut conn, &cache_key, &analytics, 60).await;
    }

    Ok(analytics)
}
