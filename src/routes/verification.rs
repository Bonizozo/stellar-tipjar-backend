use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::verification_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::verification::{ReviewVerificationRequest, SubmitVerificationRequest, VerificationResponse};

/// Public + creator routes
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators/:username/verification", post(submit_verification).get(get_verification))
}

/// Admin routes (merged into admin router separately)
pub fn admin_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    use axum::middleware;
    use crate::middleware::admin_auth::require_admin;

    Router::new()
        .route("/admin/verifications/pending", get(list_pending))
        .route("/admin/verifications/:id/review", post(review_verification))
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

async fn submit_verification(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<SubmitVerificationRequest>,
) -> Result<impl IntoResponse, AppError> {
    let v = verification_controller::submit_verification(&state.db, &username, body).await?;
    let resp: VerificationResponse = v.into();
    Ok((StatusCode::CREATED, Json(serde_json::json!(resp))).into_response())
}

async fn get_verification(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    match verification_controller::get_verification(&state.db, &username).await? {
        Some(v) => {
            let resp: VerificationResponse = v.into();
            Ok((StatusCode::OK, Json(serde_json::json!(resp))).into_response())
        }
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "No verification request found" })),
        )
            .into_response()),
    }
}

async fn list_pending(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let rows = verification_controller::list_pending(&state.db).await?;
    let resp: Vec<VerificationResponse> = rows.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(serde_json::json!(resp))).into_response())
}

async fn review_verification(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ReviewVerificationRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Resolve admin username from the API key header (already validated by middleware)
    let raw_key = headers
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let key_hash = crate::middleware::admin_auth::hash_api_key(raw_key);
    let admin_username = sqlx::query_scalar::<_, String>(
        "SELECT username FROM admin_users WHERE api_key_hash = $1",
    )
    .bind(&key_hash)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or_else(|| "admin".to_string());

    let v = verification_controller::review_verification(&state.db, id, &admin_username, body).await?;
    let resp: VerificationResponse = v.into();
    Ok((StatusCode::OK, Json(serde_json::json!(resp))).into_response())
}
