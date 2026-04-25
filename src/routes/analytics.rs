use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::controllers::analytics_controller::{self, AnalyticsQuery};
use crate::db::connection::AppState;
use crate::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct TopCreatorsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    10
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/analytics/summary", get(platform_summary))
        .route("/analytics/timeseries", get(time_series))
        .route("/analytics/top-creators", get(top_creators))
        .route("/analytics/creators/:username", get(creator_analytics))
}

async fn platform_summary(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let summary = analytics_controller::get_platform_summary(&state).await?;
    Ok((StatusCode::OK, Json(summary)))
}

async fn time_series(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AnalyticsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let points = analytics_controller::get_time_series(&state, &query).await?;
    Ok((StatusCode::OK, Json(points)))
}

async fn top_creators(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopCreatorsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let creators = analytics_controller::get_top_creators(&state, params.limit).await?;
    Ok((StatusCode::OK, Json(creators)))
}

async fn creator_analytics(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let analytics = analytics_controller::get_creator_analytics(&state, &username).await?;
    Ok((StatusCode::OK, Json(analytics)))
}
