use sqlx::PgPool;
use uuid::Uuid;
use crate::errors::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TenantAnalytics {
    pub tenant_id: Uuid,
    pub total_creators: i64,
    pub total_tips: i64,
    pub total_revenue: String,
    pub active_users: i64,
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
                $1 as tenant_id,
                COUNT(DISTINCT c.id) as total_creators,
                COUNT(DISTINCT t.id) as total_tips,
                COALESCE(SUM(t.amount), '0') as total_revenue,
                COUNT(DISTINCT t.supporter_id) as active_users,
                $2::text as period
             FROM creators c
             LEFT JOIN tips t ON c.id = t.creator_id 
                AND t.created_at > NOW() - INTERVAL '1 day' * $3
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
                $1 as tenant_id,
                COUNT(DISTINCT c.id)::i32 as creators_used,
                COALESCE(SUM(CASE WHEN t.created_at > NOW() - INTERVAL '1 day' THEN 1 ELSE 0 END), 0)::i32 as tips_today,
                COALESCE(SUM(CASE WHEN t.created_at > NOW() - INTERVAL '1 month' THEN 1 ELSE 0 END), 0)::i32 as tips_month
             FROM creators c
             LEFT JOIN tips t ON c.id = t.creator_id
             WHERE c.tenant_id = $1",
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(usage)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TenantUsage {
    pub tenant_id: Uuid,
    pub creators_used: i32,
    pub tips_today: i32,
    pub tips_month: i32,
}
