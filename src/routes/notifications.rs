use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::notification_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::notification::UpdatePreferencesRequest;

#[derive(Deserialize)]
pub struct NotificationQuery {
    #[serde(default)]
    pub unread_only: bool,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/creators/:username/notifications/preferences",
            get(get_preferences).put(update_preferences),
        )
        .route("/creators/:username/notifications", get(list_notifications))
        .route(
            "/creators/:username/notifications/read-all",
            patch(mark_all_read),
        )
        .route(
            "/creators/:username/notifications/:id/read",
            patch(mark_read),
        )
}

async fn get_preferences(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let prefs = notification_controller::get_preferences(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(prefs)))
}

async fn update_preferences(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<UpdatePreferencesRequest>,
) -> Result<impl IntoResponse, AppError> {
    let prefs = notification_controller::update_preferences(&state.db, &username, body).await?;
    Ok((StatusCode::OK, Json(prefs)))
}

async fn list_notifications(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(q): Query<NotificationQuery>,
) -> Result<impl IntoResponse, AppError> {
    let notifications =
        notification_controller::list_notifications(&state.db, &username, q.unread_only).await?;
    Ok((StatusCode::OK, Json(notifications)))
}

async fn mark_read(
    State(state): State<Arc<AppState>>,
    Path((username, id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    notification_controller::mark_read(&state.db, &username, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_all_read(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    notification_controller::mark_all_read(&state.db, &username).await?;
    Ok(StatusCode::NO_CONTENT)
}
