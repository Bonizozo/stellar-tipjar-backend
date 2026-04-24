use serde::Serialize;
use sqlx::PgPool;

use crate::errors::AppResult;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EndpointStat {
    pub path: String,
    pub method: String,
    pub request_count: i64,
    pub avg_duration_ms: Option<f64>,
    pub p95_duration_ms: Option<f64>,
    pub error_count: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SlowQuery {
    pub path: String,
    pub method: String,
    pub max_duration_ms: Option<i32>,
    pub avg_duration_ms: Option<f64>,
    pub logged_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct UsageReport {
    pub total_requests: i64,
    pub avg_duration_ms: Option<f64>,
    pub error_rate_pct: f64,
    pub top_endpoints: Vec<EndpointStat>,
    pub slow_endpoints: Vec<SlowQuery>,
}

pub async fn get_report(db: &PgPool, hours: i64) -> AppResult<UsageReport> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_logs WHERE logged_at > NOW() - ($1 || ' hours')::interval",
    )
    .bind(hours)
    .fetch_one(db)
    .await?;

    let avg_duration: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(duration_ms) FROM api_usage_logs WHERE logged_at > NOW() - ($1 || ' hours')::interval",
    )
    .bind(hours)
    .fetch_one(db)
    .await?;

    let error_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_logs WHERE status_code >= 500 AND logged_at > NOW() - ($1 || ' hours')::interval",
    )
    .bind(hours)
    .fetch_one(db)
    .await?;

    let error_rate = if total > 0 { error_count as f64 / total as f64 * 100.0 } else { 0.0 };

    let top_endpoints = sqlx::query_as::<_, EndpointStat>(
        "SELECT path, method,
                COUNT(*) AS request_count,
                AVG(duration_ms) AS avg_duration_ms,
                PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY duration_ms) AS p95_duration_ms,
                COUNT(*) FILTER (WHERE status_code >= 400) AS error_count
         FROM api_usage_logs
         WHERE logged_at > NOW() - ($1 || ' hours')::interval
         GROUP BY path, method
         ORDER BY request_count DESC
         LIMIT 20",
    )
    .bind(hours)
    .fetch_all(db)
    .await?;

    let slow_endpoints = sqlx::query_as::<_, SlowQuery>(
        "SELECT path, method, MAX(duration_ms) AS max_duration_ms,
                AVG(duration_ms) AS avg_duration_ms, MAX(logged_at) AS logged_at
         FROM api_usage_logs
         WHERE logged_at > NOW() - ($1 || ' hours')::interval
         GROUP BY path, method
         HAVING AVG(duration_ms) > 500
         ORDER BY avg_duration_ms DESC
         LIMIT 10",
    )
    .bind(hours)
    .fetch_all(db)
    .await?;

    Ok(UsageReport {
        total_requests: total,
        avg_duration_ms: avg_duration,
        error_rate_pct: error_rate,
        top_endpoints,
        slow_endpoints,
    })
}
