use axum::{
    extract::{Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::controllers::audit_log_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::middleware::admin_auth::require_admin;
use crate::models::audit_log::AuditLogQuery;

#[derive(Debug, Deserialize)]
pub struct PurgeQuery {
    /// Retain logs for this many days (default 90)
    #[serde(default = "default_retain_days")]
    pub retain_days: i64,
}

fn default_retain_days() -> i64 {
    90
}

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/audit-logs", get(search_logs))
        .route("/audit-logs/purge", delete(purge_logs))
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

async fn search_logs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AuditLogQuery>,
) -> Result<impl IntoResponse, AppError> {
    let logs = audit_log_controller::search(&state.db, &query).await?;
    Ok((StatusCode::OK, Json(logs)))
}

async fn purge_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PurgeQuery>,
) -> Result<impl IntoResponse, AppError> {
    let retain_days = params.retain_days.clamp(7, 3650);
    let deleted = audit_log_controller::purge_old_logs(&state.db, retain_days).await?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "deleted": deleted, "retain_days": retain_days })),
    ))
}
