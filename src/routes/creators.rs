 use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post, put},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::creator_controller;
use crate::controllers::tip_controller;
use crate::db::connection::AppState;
use crate::errors::{AppError, ValidationError};
use crate::models::creator::{CreateCreatorRequest, CreatorResponse, UpdateCreatorProfileRequest, UpdateCreatorWalletRequest};
use crate::models::pagination::PaginationParams;
use crate::models::tip::{TipFilters, TipResponse, TipSortParams};
use crate::search::SearchQuery;

/// Write routes: POST /creators — subject to stricter rate limiting.
pub fn write_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators", post(create_creator))
        .route(
            "/creators/:username/wallet",
            patch(update_creator_wallet),
        )
        .route(
            "/creators/:username",
            put(update_creator_profile),
        )
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
        (status = 400, description = "Validation error"),
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
    Query(filters): Query<TipFilters>,
    Query(sort): Query<TipSortParams>,
) -> Result<impl IntoResponse, AppError> {
    let result = tip_controller::get_tips_paginated(&state, Some(&username), params, filters, sort)
        .await?;
    let response = result.map(TipResponse::from);
    Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
}

/// Update a creator's wallet address with proof of ownership from the new address.
#[utoipa::path(
    patch,
    path = "/creators/{username}/wallet",
    tag = "creators",
    params(
        ("username" = String, Path, description = "Creator's unique username")
    ),
    request_body = UpdateCreatorWalletRequest,
    responses(
        (status = 200, description = "Wallet updated successfully", body = CreatorResponse),
        (status = 400, description = "Invalid request or signature"),
        (status = 404, description = "Creator not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_creator_wallet(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<UpdateCreatorWalletRequest>,
) -> Result<impl IntoResponse, AppError> {
    let creator = creator_controller::update_creator_wallet_address(&state, &username, body).await?;
    let response: CreatorResponse = creator.into();
    Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
}

/// Update a creator's profile (bio, display_name, avatar_url, social_links, categories, tags)
#[utoipa::path(
    put,
    path = "/creators/{username}/profile",
    tag = "creators",
    params(
        ("username" = String, Path, description = "Creator's unique username")
    ),
    request_body = UpdateCreatorProfileRequest,
    responses(
        (status = 200, description = "Profile updated successfully", body = CreatorResponse),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Creator not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_creator_profile(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<UpdateCreatorProfileRequest>,
) -> Result<impl IntoResponse, AppError> {
    let creator = creator_controller::update_creator_profile(&state, &username, body).await?;
    let response: CreatorResponse = creator.into();
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

    match creator_controller::search_creators(&state, &query).await {
        Ok(creators) => {
            let response: Vec<CreatorResponse> = creators.into_iter().map(Into::into).collect();
            Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
        }
        Err(e) => {
            tracing::error!("Search failed: {}", e);
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Search failed" })),
            )
                .into_response())
        }
    }
}
