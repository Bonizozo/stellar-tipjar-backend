use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::controllers::admin_controller;
use crate::db::connection::AppState;
use crate::middleware::admin_auth::require_admin;
use crate::models::admin::{AuditLogResponse, DeleteCreatorRequest};

#[derive(serde::Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

/// Returns the admin router with auth middleware applied.
/// Must be merged *before* `.with_state(state)` is called on the top-level router.
pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/stats", get(get_stats))
        .route("/admin/creators/:username", delete(delete_creator))
        .route("/admin/audit-logs", get(get_audit_logs))
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn get_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match admin_controller::get_stats(&state.db).await {
        Ok(stats) => (StatusCode::OK, Json(serde_json::json!(stats))).into_response(),
        Err(e) => {
            tracing::error!("Failed to get stats: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to retrieve statistics" })),
            )
                .into_response()
        }
    }
}

async fn delete_creator(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(username): Path<String>,
    body: Option<Json<DeleteCreatorRequest>>,
) -> impl IntoResponse {
    let admin_name = resolve_admin_from_headers(&state, &headers).await;

    match admin_controller::delete_creator(&state.db, &username).await {
        Ok(true) => {
            let reason = body.as_ref().and_then(|b| b.reason.as_deref());
            let detail = reason.map(|r| format!("reason: {}", r));
            let _ = admin_controller::write_audit_log(
                &state.db,
                &admin_name,
                "delete_creator",
                Some("creator"),
                Some(&username),
                detail.as_deref(),
            )
            .await;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": format!("Creator '{}' deleted", username) })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Creator not found" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to delete creator: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to delete creator" })),
            )
                .into_response()
        }
    }
}

async fn get_audit_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditQuery>,
) -> impl IntoResponse {
    let limit = params.limit.clamp(1, 200);
    match admin_controller::get_audit_logs(&state.db, limit).await {
        Ok(logs) => {
            let response: Vec<AuditLogResponse> = logs.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(serde_json::json!(response))).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get audit logs: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to retrieve audit logs" })),
            )
                .into_response()
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolves the admin username from the X-Admin-Key header (already validated by middleware).
async fn resolve_admin_from_headers(state: &Arc<AppState>, headers: &HeaderMap) -> String {
    let hash = headers
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .map(|k| format!("{:x}", Sha256::digest(k.as_bytes())));

    if let Some(h) = hash {
        admin_controller::get_admin_username_by_key_hash(&state.db, &h)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    }
}
