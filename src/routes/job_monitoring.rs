use crate::db::connection::AppState;
use crate::jobs::monitoring::JobMonitor;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;

/// GET /jobs/dashboard
async fn dashboard(
    State((_state, monitor)): State<(Arc<AppState>, Arc<JobMonitor>)>,
) -> impl IntoResponse {
    match monitor.dashboard().await {
        Ok(dash) => (StatusCode::OK, Json(dash)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Job dashboard error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to collect job metrics"})),
            )
                .into_response()
        }
    }
}

/// GET /jobs/metrics
async fn metrics(
    State((_state, monitor)): State<(Arc<AppState>, Arc<JobMonitor>)>,
) -> impl IntoResponse {
    match monitor.dashboard().await {
        Ok(dash) => (
            StatusCode::OK,
            Json(json!({
                "pending":   dash.queue_depth.pending,
                "running":   dash.queue_depth.running,
                "retrying":  dash.queue_depth.retrying,
                "completed": dash.queue_depth.completed,
                "failed":    dash.queue_depth.failed,
                "oldest_pending_age_secs": dash.oldest_pending_age_secs,
                "alerts":    dash.alerts.len(),
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Job metrics error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "failed to collect job metrics"})),
            )
                .into_response()
        }
    }
}

pub fn router(state: Arc<AppState>, monitor: Arc<JobMonitor>) -> Router {
    Router::new()
        .route("/jobs/dashboard", get(dashboard))
        .route("/jobs/metrics", get(metrics))
        .with_state((state, monitor))
}
