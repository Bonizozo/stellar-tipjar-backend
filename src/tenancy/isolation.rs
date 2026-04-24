use crate::errors::app_error::AppError;
use crate::models::creator::Creator;
use crate::tenancy::context::TenantContext;
use sqlx::PgPool;

pub struct TenantAwareQuery;

impl TenantAwareQuery {
    pub async fn get_creators(
        pool: &PgPool,
        tenant: &TenantContext,
    ) -> Result<Vec<Creator>, AppError> {
        sqlx::query_as::<_, Creator>(
            "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE tenant_id = $1"
        )
        .bind(tenant.tenant_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))
    }

    pub async fn get_creator_by_username(
        pool: &PgPool,
        tenant: &TenantContext,
        username: &str,
    ) -> Result<Option<Creator>, AppError> {
        sqlx::query_as::<_, Creator>(
            "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE tenant_id = $1 AND username = $2"
        )
        .bind(tenant.tenant_id)
        .bind(username)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))
    }

    pub async fn count_creators(
        pool: &PgPool,
        tenant: &TenantContext,
    ) -> Result<i64, AppError> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM creators WHERE tenant_id = $1"
        )
        .bind(tenant.tenant_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(result.0)
    }
}
