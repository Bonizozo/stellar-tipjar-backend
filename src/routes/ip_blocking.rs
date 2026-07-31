use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::ip_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::ip_block::{BlockCountryRequest, BlockIpRequest};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/ip-blocks", get(list_blocked_ips).post(block_ip))
        .route("/admin/ip-blocks/:ip", delete(unblock_ip))
        .route(
            "/admin/country-blocks",
            get(list_blocked_countries).post(block_country),
        )
        .route("/admin/country-blocks/:code", delete(unblock_country))
}

async fn list_blocked_ips(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let blocks = ip_controller::list_blocked_ips(&state.db).await?;
    Ok((StatusCode::OK, Json(blocks)))
}

async fn block_ip(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BlockIpRequest>,
) -> Result<impl IntoResponse, AppError> {
    let block = ip_controller::block_ip(&state.db, body).await?;
    Ok((StatusCode::CREATED, Json(block)))
}

async fn unblock_ip(
    State(state): State<Arc<AppState>>,
    Path(ip): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    ip_controller::unblock_ip(&state.db, &ip).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_blocked_countries(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let blocks = ip_controller::list_blocked_countries(&state.db).await?;
    Ok((StatusCode::OK, Json(blocks)))
}

async fn block_country(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BlockCountryRequest>,
) -> Result<impl IntoResponse, AppError> {
    let block = ip_controller::block_country(&state.db, body).await?;
    Ok((StatusCode::CREATED, Json(block)))
}

async fn unblock_country(
    State(state): State<Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    ip_controller::unblock_country(&state.db, &code).await?;
    Ok(StatusCode::NO_CONTENT)
}
