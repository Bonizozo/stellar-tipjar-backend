use sqlx::PgPool;
use uuid::Uuid;
use crate::errors::AppError;

/// Aggregated analytics for a tenant over a time window
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, utoipa::ToSchema)]
pub struct TenantAnalytics {
    pub tenant_id: Uuid,
    pub total_creators: i64,
    pub total_tips: i64,
    pub total_revenue: String,
    pub period: String,
}

pub struct TenantAnalyticsService {
    pool: PgPool,
}

impl TenantAnalyticsService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_tenant_analytics(
        &self,
        tenant_id: Uuid,
        days: i32,
    ) -> Result<TenantAnalytics, AppError> {
        let analytics = sqlx::query_as::<_, TenantAnalytics>(
            "SELECT
                $1::uuid AS tenant_id,
                COUNT(DISTINCT c.id) AS total_creators,
                COUNT(DISTINCT t.id) AS total_tips,
                COALESCE(SUM(t.amount::numeric), 0)::text AS total_revenue,
                $2::text AS period
             FROM creators c
             LEFT JOIN tips t ON c.username = t.creator_username
                AND t.created_at > NOW() - ($3::int * INTERVAL '1 day')
             WHERE c.tenant_id = $1",
        )
        .bind(tenant_id)
        .bind(format!("{}d", days))
        .bind(days)
        .fetch_one(&self.pool)
        .await?;

        Ok(analytics)
    }

    pub async fn get_tenant_usage(
        &self,
        tenant_id: Uuid,
    ) -> Result<TenantUsage, AppError> {
        let usage = sqlx::query_as::<_, TenantUsage>(
            "SELECT
                $1::uuid AS tenant_id,
                COUNT(DISTINCT c.id)::int AS creators_used,
                COALESCE(SUM(CASE WHEN t.created_at > NOW() - INTERVAL '1 day' THEN 1 ELSE 0 END), 0)::int AS tips_today,
                COALESCE(SUM(CASE WHEN t.created_at > NOW() - INTERVAL '30 days' THEN 1 ELSE 0 END), 0)::int AS tips_month
             FROM creators c
             LEFT JOIN tips t ON c.username = t.creator_username
             WHERE c.tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(usage)
    }
}

/// Current resource usage for a tenant
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, utoipa::ToSchema)]
pub struct TenantUsage {
    pub tenant_id: Uuid,
    pub creators_used: i32,
    pub tips_today: i32,
    pub tips_month: i32,
}
