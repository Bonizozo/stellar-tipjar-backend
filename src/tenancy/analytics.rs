use crate::errors::app_error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantAnalytics {
    pub tenant_id: Uuid,
    pub period: String,
    pub total_tips: i64,
    pub total_revenue: String,
    pub active_creators: i64,
    pub active_supporters: i64,
    pub api_calls: i64,
    pub storage_used_mb: i64,
    pub recorded_at: DateTime<Utc>,
}

pub struct TenantAnalyticsCollector {
    pool: PgPool,
}

impl TenantAnalyticsCollector {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record_analytics(
        &self,
        tenant_id: Uuid,
        period: &str,
    ) -> Result<TenantAnalytics, AppError> {
        let now = Utc::now();

        let total_tips: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tips WHERE tenant_id = $1 AND created_at > NOW() - INTERVAL '1 day'"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let total_revenue: String = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), '0') FROM tips WHERE tenant_id = $1 AND created_at > NOW() - INTERVAL '1 day'"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|_| "0".to_string());

        let active_creators: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT creator_id) FROM tips WHERE tenant_id = $1 AND created_at > NOW() - INTERVAL '1 day'"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let active_supporters: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT supporter_id) FROM tips WHERE tenant_id = $1 AND created_at > NOW() - INTERVAL '1 day'"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let api_calls: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_usage WHERE tenant_id = $1 AND created_at > NOW() - INTERVAL '1 day'"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let storage_used_mb: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size_bytes) / 1024 / 1024, 0) FROM uploads WHERE tenant_id = $1"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        sqlx::query(
            r#"
            INSERT INTO tenant_analytics (tenant_id, period, total_tips, total_revenue, active_creators, active_supporters, api_calls, storage_used_mb, recorded_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(tenant_id)
        .bind(period)
        .bind(total_tips)
        .bind(&total_revenue)
        .bind(active_creators)
        .bind(active_supporters)
        .bind(api_calls)
        .bind(storage_used_mb)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(TenantAnalytics {
            tenant_id,
            period: period.to_string(),
            total_tips,
            total_revenue,
            active_creators,
            active_supporters,
            api_calls,
            storage_used_mb,
            recorded_at: now,
        })
    }

    pub async fn get_analytics(
        &self,
        tenant_id: Uuid,
        period: &str,
    ) -> Result<TenantAnalytics, AppError> {
        sqlx::query_as::<_, TenantAnalytics>(
            "SELECT tenant_id, period, total_tips, total_revenue, active_creators, active_supporters, api_calls, storage_used_mb, recorded_at FROM tenant_analytics WHERE tenant_id = $1 AND period = $2 ORDER BY recorded_at DESC LIMIT 1"
        )
        .bind(tenant_id)
        .bind(period)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| AppError::not_found("Analytics not found".to_string()))
    }
}
