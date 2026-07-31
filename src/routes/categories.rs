use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::category_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::category::{
    AssignCategoriesRequest, AssignTagsRequest, CreateCategoryRequest, TagSearchQuery,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/categories", get(list_categories).post(create_category))
        .route("/categories/:slug/creators", get(creators_by_category))
        .route(
            "/creators/:username/categories",
            get(get_creator_categories).put(assign_categories),
        )
        .route(
            "/creators/:username/tags",
            get(get_creator_tags).put(assign_tags),
        )
        .route("/tags/search", get(search_by_tag))
}

async fn list_categories(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let cats = category_controller::list_categories(&state.db).await?;
    Ok((StatusCode::OK, Json(cats)))
}

async fn create_category(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateCategoryRequest>,
) -> Result<impl IntoResponse, AppError> {
    let cat = category_controller::create_category(&state.db, body).await?;
    Ok((StatusCode::CREATED, Json(cat)))
}

async fn creators_by_category(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let creators = category_controller::get_creators_by_category(&state.db, &slug).await?;
    Ok((StatusCode::OK, Json(creators)))
}

async fn get_creator_categories(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let cats = category_controller::get_creator_categories(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(cats)))
}

async fn assign_categories(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<AssignCategoriesRequest>,
) -> Result<impl IntoResponse, AppError> {
    category_controller::assign_categories(&state.db, &username, body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_creator_tags(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let tags = category_controller::get_creator_tags(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(tags)))
}

async fn assign_tags(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<AssignTagsRequest>,
) -> Result<impl IntoResponse, AppError> {
    let tags = category_controller::assign_tags(&state.db, &username, body).await?;
    Ok((StatusCode::OK, Json(tags)))
}

async fn search_by_tag(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TagSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    let creators = category_controller::search_by_tag(&state.db, &q.tag).await?;
    Ok((StatusCode::OK, Json(creators)))
}
