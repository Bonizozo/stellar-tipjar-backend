use crate::errors::app_error::AppError;
use crate::tenancy::context::{ResourceQuotas, TenantConfig, TenantContext};
use axum::http::Request;
use sqlx::PgPool;
use uuid::Uuid;

pub struct TenantResolver {
    pool: PgPool,
}

impl TenantResolver {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn resolve_from_request<B>(
        &self,
        req: &Request<B>,
    ) -> Result<TenantContext, AppError> {
        let tenant_id = self.extract_tenant_id(req)?;

        let tenant: (Uuid, String, String, String) =
            sqlx::query_as("SELECT id, name, config, quotas FROM tenants WHERE id = $1")
                .bind(tenant_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|_| AppError::not_found("Tenant not found"))?;

        let config: TenantConfig = serde_json::from_str(&tenant.2)
            .map_err(|_| AppError::database_error("Invalid config"))?;
        let quotas: ResourceQuotas = serde_json::from_str(&tenant.3)
            .map_err(|_| AppError::database_error("Invalid quotas"))?;

        Ok(TenantContext {
            tenant_id: tenant.0,
            tenant_name: tenant.1,
            config,
            quotas,
        })
    }

    fn extract_tenant_id<B>(&self, req: &Request<B>) -> Result<Uuid, AppError> {
        if let Some(header) = req.headers().get("X-Tenant-ID") {
            if let Ok(tenant_str) = header.to_str() {
                return Uuid::parse_str(tenant_str)
                    .map_err(|_| AppError::bad_request("Invalid tenant ID"));
            }
        }

        if let Some(host) = req.headers().get("Host") {
            if let Ok(host_str) = host.to_str() {
                if let Some(subdomain) = host_str.split('.').next() {
                    if let Ok(tenant_id) = Uuid::parse_str(subdomain) {
                        return Ok(tenant_id);
                    }
                }
            }
        }

        Err(AppError::bad_request("Tenant ID not found"))
    }
}
