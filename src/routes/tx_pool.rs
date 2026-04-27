use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::tx_pool::{EnqueueTxRequest, TxPoolStatusResponse};
use crate::services::tx_pool_service;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tx-pool", post(enqueue))
        .route("/tx-pool/stats", get(stats))
        .route("/tx-pool/:id", get(get_by_id))
        .route("/tx-pool/hash/:tx_hash", get(get_by_hash))
}

async fn enqueue(
    State(state): State<Arc<AppState>>,
    Json(body): Json<EnqueueTxRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.transaction_hash.trim().is_empty() {
        return Err(AppError::bad_request("transaction_hash is required"));
    }
    let entry = tx_pool_service::enqueue(&state, body).await?;
    Ok((StatusCode::CREATED, Json(TxPoolStatusResponse::from(entry))))
}

async fn get_by_id(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    match tx_pool_service::get_by_id(&state, id).await? {
        Some(e) => Ok((StatusCode::OK, Json(TxPoolStatusResponse::from(e))).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response()),
    }
}

async fn get_by_hash(
    State(state): State<Arc<AppState>>,
    Path(tx_hash): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    match tx_pool_service::get_by_hash(&state, &tx_hash).await? {
        Some(e) => Ok((StatusCode::OK, Json(TxPoolStatusResponse::from(e))).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response()),
    }
}

async fn stats(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let s = tx_pool_service::stats(&state).await?;
    Ok((StatusCode::OK, Json(s)))
}
