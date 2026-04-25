use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::leaderboard_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::leaderboard::LeaderboardQuery;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/leaderboard/:board_type", get(get_leaderboard))
        .route("/leaderboard/:board_type/refresh", axum::routing::post(refresh_leaderboard))
}

/// Get leaderboard entries for a board type (top_creators, top_tippers, trending)
async fn get_leaderboard(
    State(state): State<Arc<AppState>>,
    Path(board_type): Path<String>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<impl IntoResponse, AppError> {
    let valid_types = ["top_creators", "top_tippers", "trending"];
    if !valid_types.contains(&board_type.as_str()) {
        return Err(AppError::Validation(
            crate::errors::ValidationError::InvalidRequest {
                message: format!(
                    "Invalid board_type '{}'. Must be one of: top_creators, top_tippers, trending",
                    board_type
                ),
            },
        ));
    }

    let result = leaderboard_controller::get_leaderboard(
        &state.db,
        query.validated_period(),
        &board_type,
        query.validated_limit(),
    )
    .await?;

    Ok((StatusCode::OK, Json(serde_json::json!(result))).into_response())
}

/// Admin-only: trigger a leaderboard refresh
async fn refresh_leaderboard(
    State(state): State<Arc<AppState>>,
    Path(_board_type): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    leaderboard_controller::refresh_leaderboards(&state.db).await?;

    // Invalidate cached leaderboard HTTP responses and entity cache entries
    if let Some(ref inv) = state.invalidator {
        let _ = inv.invalidate_pattern("leaderboard:*").await;
        let _ = inv.invalidate_pattern(&crate::cache::keys::http_response_pattern("/leaderboard/")).await;
    }

    Ok((StatusCode::OK, Json(serde_json::json!({ "status": "refreshed" }))).into_response())
}
