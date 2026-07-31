use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::follow_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators/:username/followers", get(get_followers))
        .route("/creators/:username/following", get(get_following))
        .route("/creators/:username/follow-counts", get(get_follow_counts))
        .route("/creators/:username/feed", get(get_feed))
        .route("/creators/:follower/follow/:followed", post(follow))
        .route("/creators/:follower/follow/:followed", delete(unfollow))
}

async fn get_followers(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let follows = follow_controller::get_followers(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(follows)))
}

async fn get_following(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let follows = follow_controller::get_following(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(follows)))
}

async fn get_follow_counts(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let counts = follow_controller::get_follow_counts(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(counts)))
}

async fn get_feed(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let feed = follow_controller::get_feed(&state.db, &username, 50).await?;
    Ok((StatusCode::OK, Json(feed)))
}

async fn follow(
    State(state): State<Arc<AppState>>,
    Path((follower, followed)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    if follower == followed {
        return Err(AppError::Validation(
            crate::errors::ValidationError::InvalidRequest {
                message: "Cannot follow yourself".to_string(),
            },
        ));
    }
    let f = follow_controller::follow(&state.db, &follower, &followed).await?;
    Ok((StatusCode::CREATED, Json(f)))
}

async fn unfollow(
    State(state): State<Arc<AppState>>,
    Path((follower, followed)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    follow_controller::unfollow(&state.db, &follower, &followed).await?;
    Ok(StatusCode::NO_CONTENT)
}
