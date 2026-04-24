use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::api_key_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::api_key::CreateApiKeyRequest;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/api-keys", post(create).get(list))
        .route("/api-keys/:key", get(get_key))
        .route("/api-keys/:key/rotate", post(rotate))
        .route("/api-keys/:key/revoke", delete(revoke))
        .with_state(state)
}

async fn create(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<impl IntoResponse, AppError> {
    let created = api_key_controller::create_api_key(&state, body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn list(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, AppError> {
    let keys = api_key_controller::list_api_keys(&state).await?;
    Ok((StatusCode::OK, Json(keys)))
}

async fn get_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let view = api_key_controller::get_api_key(&state, &key).await?;
    Ok((StatusCode::OK, Json(view)))
}

async fn rotate(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let created = api_key_controller::rotate_api_key(&state, &key).await?;
    Ok((StatusCode::OK, Json(created)))
}

async fn revoke(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    api_key_controller::revoke_api_key(&state, &key).await?;
    Ok(StatusCode::NO_CONTENT)
}
