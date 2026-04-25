use crate::errors::app_error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantProvisionRequest {
    pub name: String,
    pub organization_id: String,
    pub tier: TenantTier,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TenantTier {
    Free,
    Professional,
    Enterprise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionedTenant {
    pub id: Uuid,
    pub name: String,
    pub organization_id: String,
    pub tier: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub provisioned_at: DateTime<Utc>,
}

pub struct TenantProvisioner {
    pool: PgPool,
}

impl TenantProvisioner {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn provision_tenant(
        &self,
        req: TenantProvisionRequest,
    ) -> Result<ProvisionedTenant, AppError> {
        let tenant_id = Uuid::new_v4();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO tenants (id, name, organization_id, tier, status, config, created_at, provisioned_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(tenant_id)
        .bind(&req.name)
        .bind(&req.organization_id)
        .bind(format!("{:?}", req.tier))
        .bind("active")
        .bind(req.config)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(ProvisionedTenant {
            id: tenant_id,
            name: req.name,
            organization_id: req.organization_id,
            tier: format!("{:?}", req.tier),
            status: "active".to_string(),
            created_at: now,
            provisioned_at: now,
        })
    }

    pub async fn get_tenant(&self, tenant_id: Uuid) -> Result<ProvisionedTenant, AppError> {
        sqlx::query_as::<_, ProvisionedTenant>(
            "SELECT id, name, organization_id, tier, status, created_at, provisioned_at FROM tenants WHERE id = $1"
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| AppError::not_found("Tenant not found".to_string()))
    }

    pub async fn list_tenants_by_org(
        &self,
        org_id: &str,
    ) -> Result<Vec<ProvisionedTenant>, AppError> {
        sqlx::query_as::<_, ProvisionedTenant>(
            "SELECT id, name, organization_id, tier, status, created_at, provisioned_at FROM tenants WHERE organization_id = $1 ORDER BY created_at DESC"
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))
    }

    pub async fn deprovision_tenant(&self, tenant_id: Uuid) -> Result<(), AppError> {
        sqlx::query("UPDATE tenants SET status = 'inactive' WHERE id = $1")
            .bind(tenant_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(())
    }
}
