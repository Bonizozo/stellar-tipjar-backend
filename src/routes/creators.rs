use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::creator_controller;
use crate::controllers::tip_controller;
use crate::db::connection::AppState;
use crate::errors::{AppError, ValidationError};
use crate::models::creator::{CreateCreatorRequest, CreatorResponse};
use crate::models::pagination::PaginationParams;
use crate::models::tip::TipResponse;
use crate::search::SearchQuery;

/// Write routes: POST /creators — subject to stricter rate limiting.
pub fn write_router() -> Router<Arc<AppState>> {
    Router::new().route("/creators", post(create_creator))
}

/// Read routes: GET /creators/:username, GET /creators/:username/tips — general rate limiting.
pub fn read_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators/search", get(search_creators))
        .route("/creators/:username", get(get_creator))
        .route("/creators/:username/tips", get(get_creator_tips))
}

/// Create a new creator profile
#[utoipa::path(
    post,
    path = "/creators",
    tag = "creators",
    request_body = CreateCreatorRequest,
    responses(
        (status = 201, description = "Creator created successfully", body = CreatorResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_creator(
    State(state): State<Arc<AppState>>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<CreateCreatorRequest>,
) -> Result<impl IntoResponse, AppError> {
    let creator = creator_controller::create_creator(&state, body).await?;
    let response: CreatorResponse = creator.into();
    Ok((StatusCode::CREATED, Json(serde_json::json!(response))).into_response())
}

/// Get a creator by username
#[utoipa::path(
    get,
    path = "/creators/{username}",
    tag = "creators",
    params(
        ("username" = String, Path, description = "Creator's unique username")
    ),
    responses(
        (status = 200, description = "Creator found", body = CreatorResponse),
        (status = 404, description = "Creator not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_creator(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let creator = creator_controller::get_creator_or_not_found(&state, &username).await?;
    let response: CreatorResponse = creator.into();
    Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
}

/// List tips for a creator with pagination
#[utoipa::path(
    get,
    path = "/creators/{username}/tips",
    tag = "creators",
    params(
        ("username" = String, Path, description = "Creator's unique username"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "Paginated list of tips"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_creator_tips(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    let _ = params;
    let tips = tip_controller::get_tips_for_creator(&state, &username).await?;
    let response: Vec<TipResponse> = tips.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
}

/// Search creators by username
#[utoipa::path(
    get,
    path = "/creators/search",
    tag = "creators",
    params(SearchQuery),
    responses(
        (status = 200, description = "Search results", body = Vec<CreatorResponse>),
        (status = 400, description = "Missing or invalid query parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn search_creators(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    if query.q.trim().is_empty() {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "Query parameter 'q' must not be empty".to_string(),
        }));
    }

    let creators = creator_controller::search_creators(&state.db, &query).await?;
    let response: Vec<CreatorResponse> = creators.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
}
