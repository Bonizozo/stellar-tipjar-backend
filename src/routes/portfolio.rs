use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::portfolio_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::portfolio::{
    CreatePortfolioItemRequest, ReorderRequest, UpdatePortfolioItemRequest,
};
use crate::validation::ValidatedJson;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/creators/:username/portfolio",
            get(list_items).post(create_item),
        )
        .route(
            "/creators/:username/portfolio/reorder",
            post(reorder_items),
        )
        .route(
            "/creators/:username/portfolio/:id",
            get(get_item).put(update_item).delete(delete_item),
        )
}

async fn list_items(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let items = portfolio_controller::list_items(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(items)))
}

async fn get_item(
    State(state): State<Arc<AppState>>,
    Path((username, id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let item = portfolio_controller::get_item(&state.db, id, &username).await?;
    Ok((StatusCode::OK, Json(item)))
}

async fn create_item(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    ValidatedJson(body): ValidatedJson<CreatePortfolioItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    let item = portfolio_controller::create_item(&state.db, &username, body).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update_item(
    State(state): State<Arc<AppState>>,
    Path((username, id)): Path<(String, Uuid)>,
    ValidatedJson(body): ValidatedJson<UpdatePortfolioItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    let item = portfolio_controller::update_item(&state.db, id, &username, body).await?;
    Ok((StatusCode::OK, Json(item)))
}

async fn delete_item(
    State(state): State<Arc<AppState>>,
    Path((username, id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    portfolio_controller::delete_item(&state.db, id, &username).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reorder_items(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<ReorderRequest>,
) -> Result<impl IntoResponse, AppError> {
    let items = portfolio_controller::reorder_items(&state.db, &username, body.ids).await?;
    Ok((StatusCode::OK, Json(items)))
}
