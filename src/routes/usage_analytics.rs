use axum::{
    extract::{Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::controllers::usage_analytics_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::middleware::admin_auth::require_admin;

#[derive(Deserialize)]
struct ReportQuery {
    #[serde(default = "default_hours")]
    hours: i64,
}

fn default_hours() -> i64 {
    24
}

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/analytics/usage", get(usage_report))
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

async fn usage_report(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ReportQuery>,
) -> impl IntoResponse {
    let hours = q.hours.clamp(1, 720);
    match usage_analytics_controller::get_report(&state.db, hours).await {
        Ok(report) => (StatusCode::OK, Json(serde_json::json!(report))).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}
