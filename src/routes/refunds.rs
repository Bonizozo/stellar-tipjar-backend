use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::{admin_controller, refund_controller};
use crate::db::connection::AppState;
use crate::middleware::admin_auth::require_admin;
use crate::models::refund::{CreateRefundRequest, ReviewRefundRequest};

#[derive(Deserialize)]
struct ListQuery {
    status: Option<String>,
}

/// Admin-protected refund management routes.
pub fn admin_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/refunds", get(list_refunds))
        .route("/admin/refunds/:id/review", post(review_refund))
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

/// Public refund request routes (no auth required).
pub fn public_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/refunds", post(create_refund))
        .route("/refunds/:id", get(get_refund))
}

async fn create_refund(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRefundRequest>,
) -> impl IntoResponse {
    match refund_controller::create_refund(&state.db, body).await {
        Ok(refund) => (StatusCode::CREATED, Json(serde_json::json!(refund))).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn get_refund(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match refund_controller::get_refund(&state.db, id).await {
        Ok(Some(r)) => (StatusCode::OK, Json(serde_json::json!(r))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Refund not found" })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn list_refunds(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    match refund_controller::list_refunds(&state.db, q.status.as_deref()).await {
        Ok(refunds) => (StatusCode::OK, Json(serde_json::json!(refunds))).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn review_refund(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<ReviewRefundRequest>,
) -> impl IntoResponse {
    let reviewer = resolve_admin(&state, &headers).await;
    match refund_controller::review_refund(&state.db, id, &reviewer, body).await {
        Ok(Some(r)) => (StatusCode::OK, Json(serde_json::json!(r))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Refund not found or not pending" })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn resolve_admin(state: &Arc<AppState>, headers: &HeaderMap) -> String {
    let hash = headers
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .map(|k| {
            Sha256::digest(k.as_bytes())
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>()
        });
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
