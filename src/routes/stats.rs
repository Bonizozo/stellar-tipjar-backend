use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::stats_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::stats::StatsQuery;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators/:username/stats", get(get_stats))
        .route("/creators/:username/stats/summary", get(get_summary))
        .route("/creators/:username/stats/daily", get(get_daily))
        .route("/creators/:username/stats/aggregate", post(aggregate))
}

async fn get_stats(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let stats = stats_controller::get_creator_stats(&state, &username).await?;
    Ok((StatusCode::OK, Json(stats)))
}

async fn get_summary(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let summary = stats_controller::get_creator_summary(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(summary)))
}

async fn get_daily(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(q): Query<StatsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let stats = stats_controller::get_daily_stats(&state.db, &username, &q).await?;
    Ok((StatusCode::OK, Json(stats)))
}

async fn aggregate(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    stats_controller::aggregate_daily_stats(&state.db, &username).await?;
    Ok(StatusCode::NO_CONTENT)
}
