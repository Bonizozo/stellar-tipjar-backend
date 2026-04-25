use crate::errors::AppError;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TenantProvisioner {
    pool: PgPool,
}

impl TenantProvisioner {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn provision_tenant(
        &self,
        tenant_id: Uuid,
        tenant_name: &str,
        max_creators: i32,
        max_tips_per_day: i32,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO tenants (id, name, max_creators, max_tips_per_day, created_at) 
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(tenant_id)
        .bind(tenant_name)
        .bind(max_creators)
        .bind(max_tips_per_day)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO tenant_configs (tenant_id, features, custom_domain, created_at) 
             VALUES ($1, $2, NULL, NOW())",
        )
        .bind(tenant_id)
        .bind("[]")
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn deprovision_tenant(&self, tenant_id: Uuid) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM tenant_configs WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM tenants WHERE id = $1")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn update_tenant_quotas(
        &self,
        tenant_id: Uuid,
        max_creators: i32,
        max_tips_per_day: i32,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE tenants SET max_creators = $1, max_tips_per_day = $2 WHERE id = $3",
        )
        .bind(max_creators)
        .bind(max_tips_per_day)
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
