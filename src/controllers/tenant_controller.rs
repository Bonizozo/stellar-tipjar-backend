use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::tenant::{CreateTenantRequest, Tenant, TenantResponse, UpdateTenantRequest};
use crate::tenancy::{TenantAnalytics, TenantAnalyticsService, TenantUsage};

pub async fn create_tenant(
    state: &AppState,
    req: &CreateTenantRequest,
) -> Result<TenantResponse, AppError> {
    let mut tx = state.db.begin().await?;

    let tenant = sqlx::query_as::<_, Tenant>(
        "INSERT INTO tenants (name, slug, max_creators, max_tips_per_day)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(&req.name)
    .bind(&req.slug)
    .bind(req.max_creators.unwrap_or(100))
    .bind(req.max_tips_per_day.unwrap_or(10000))
    .fetch_one(&mut *tx)
    .await?;

    // Insert default tenant config
    sqlx::query(
        "INSERT INTO tenant_configs (tenant_id, features, custom_domain, created_at)
         VALUES ($1, '[]', NULL, NOW())
         ON CONFLICT (tenant_id) DO NOTHING",
    )
    .bind(tenant.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(tenant.into())
}

pub async fn list_tenants(state: &AppState) -> Result<Vec<TenantResponse>, AppError> {
    let tenants = sqlx::query_as::<_, Tenant>(
        "SELECT * FROM tenants ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(tenants.into_iter().map(Into::into).collect())
}

pub async fn get_tenant(state: &AppState, tenant_id: Uuid) -> Result<TenantResponse, AppError> {
    let tenant = sqlx::query_as::<_, Tenant>("SELECT * FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Tenant not found"))?;

    Ok(tenant.into())
}

pub async fn update_tenant(
    state: &AppState,
    tenant_id: Uuid,
    req: &UpdateTenantRequest,
) -> Result<TenantResponse, AppError> {
    let tenant = sqlx::query_as::<_, Tenant>(
        "UPDATE tenants
         SET name             = COALESCE($1, name),
             max_creators     = COALESCE($2, max_creators),
             max_tips_per_day = COALESCE($3, max_tips_per_day),
             is_active        = COALESCE($4, is_active),
             updated_at       = NOW()
         WHERE id = $5
         RETURNING *",
    )
    .bind(&req.name)
    .bind(req.max_creators)
    .bind(req.max_tips_per_day)
    .bind(req.is_active)
    .bind(tenant_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Tenant not found"))?;

    Ok(tenant.into())
}

pub async fn delete_tenant(state: &AppState, tenant_id: Uuid) -> Result<(), AppError> {
    // tenant_configs cascade-deletes via FK; delete tenant row directly
    let result = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Tenant not found"));
    }
    Ok(())
}

pub async fn get_tenant_analytics(
    state: &AppState,
    tenant_id: Uuid,
    days: i32,
) -> Result<TenantAnalytics, AppError> {
    let svc = TenantAnalyticsService::new(state.db.clone());
    svc.get_tenant_analytics(tenant_id, days).await
}

pub async fn get_tenant_usage(
    state: &AppState,
    tenant_id: Uuid,
) -> Result<TenantUsage, AppError> {
    let svc = TenantAnalyticsService::new(state.db.clone());
    svc.get_tenant_usage(tenant_id).await
}
