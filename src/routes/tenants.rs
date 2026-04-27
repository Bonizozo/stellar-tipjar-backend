use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use crate::controllers::tenant_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::tenant::{CreateTenantRequest, TenantResponse, UpdateTenantRequest};
use crate::tenancy::{TenantAnalytics, TenantUsage};

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AnalyticsDaysQuery {
    /// Number of days to include in the analytics window (default 30)
    #[serde(default = "default_days")]
    pub days: i32,
}

fn default_days() -> i32 {
    30
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tenants", post(create_tenant).get(list_tenants))
        .route(
            "/tenants/:id",
            get(get_tenant).put(update_tenant).delete(delete_tenant),
        )
        .route("/tenants/:id/analytics", get(get_analytics))
        .route("/tenants/:id/usage", get(get_usage))
}

/// Provision a new tenant organisation
#[utoipa::path(
    post,
    path = "/tenants",
    tag = "tenants",
    request_body(
        content = CreateTenantRequest,
        example = json!({
            "name": "Acme Corp",
            "slug": "acme-corp",
            "max_creators": 200,
            "max_tips_per_day": 5000
        })
    ),
    responses(
        (status = 201, description = "Tenant provisioned", body = TenantResponse,
         example = json!({
             "id": "550e8400-e29b-41d4-a716-446655440000",
             "name": "Acme Corp",
             "slug": "acme-corp",
             "max_creators": 200,
             "max_tips_per_day": 5000,
             "is_active": true,
             "created_at": "2024-03-14T10:30:00Z"
         })),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Slug already taken")
    )
)]
async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTenantRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(|e| AppError::bad_request(e.to_string()))?;
    let tenant = tenant_controller::create_tenant(&state, &req).await?;
    Ok((StatusCode::CREATED, Json(tenant)))
}

/// List all tenants
#[utoipa::path(
    get,
    path = "/tenants",
    tag = "tenants",
    responses(
        (status = 200, description = "List of tenants", body = Vec<TenantResponse>)
    )
)]
async fn list_tenants(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let tenants = tenant_controller::list_tenants(&state).await?;
    Ok((StatusCode::OK, Json(tenants)))
}

/// Get a tenant by ID
#[utoipa::path(
    get,
    path = "/tenants/{id}",
    tag = "tenants",
    params(("id" = Uuid, Path, description = "Tenant UUID")),
    responses(
        (status = 200, description = "Tenant found", body = TenantResponse),
        (status = 404, description = "Tenant not found")
    )
)]
async fn get_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let tenant = tenant_controller::get_tenant(&state, id).await?;
    Ok((StatusCode::OK, Json(tenant)))
}

/// Update tenant quotas or status
#[utoipa::path(
    put,
    path = "/tenants/{id}",
    tag = "tenants",
    params(("id" = Uuid, Path, description = "Tenant UUID")),
    request_body = UpdateTenantRequest,
    responses(
        (status = 200, description = "Tenant updated", body = TenantResponse),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Tenant not found")
    )
)]
async fn update_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTenantRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(|e| AppError::bad_request(e.to_string()))?;
    let tenant = tenant_controller::update_tenant(&state, id, &req).await?;
    Ok((StatusCode::OK, Json(tenant)))
}

/// Deprovision a tenant and all its data
#[utoipa::path(
    delete,
    path = "/tenants/{id}",
    tag = "tenants",
    params(("id" = Uuid, Path, description = "Tenant UUID")),
    responses(
        (status = 204, description = "Tenant deleted"),
        (status = 404, description = "Tenant not found")
    )
)]
async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    tenant_controller::delete_tenant(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Get analytics for a tenant over a time window
#[utoipa::path(
    get,
    path = "/tenants/{id}/analytics",
    tag = "tenants",
    params(
        ("id" = Uuid, Path, description = "Tenant UUID"),
        AnalyticsDaysQuery
    ),
    responses(
        (status = 200, description = "Tenant analytics", body = TenantAnalytics),
        (status = 404, description = "Tenant not found")
    )
)]
async fn get_analytics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(q): Query<AnalyticsDaysQuery>,
) -> Result<impl IntoResponse, AppError> {
    let analytics = tenant_controller::get_tenant_analytics(&state, id, q.days).await?;
    Ok((StatusCode::OK, Json(analytics)))
}

/// Get current resource usage for a tenant
#[utoipa::path(
    get,
    path = "/tenants/{id}/usage",
    tag = "tenants",
    params(("id" = Uuid, Path, description = "Tenant UUID")),
    responses(
        (status = 200, description = "Tenant usage", body = TenantUsage),
        (status = 404, description = "Tenant not found")
    )
)]
async fn get_usage(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let usage = tenant_controller::get_tenant_usage(&state, id).await?;
    Ok((StatusCode::OK, Json(usage)))
}
