use crate::db::connection::AppState;
use crate::scheduler::JobMonitor;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub struct JobStatusResponse {
    pub name: String,
    pub last_run: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub run_count: i64,
    pub failure_count: i64,
    pub avg_duration_ms: Option<i64>,
}

/// Get all job statuses
pub async fn get_all_jobs(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let monitor = JobMonitor::new(state.db.clone());

    match monitor.get_all_job_statuses().await {
        Ok(statuses) => {
            let response: Vec<JobStatusResponse> = statuses
                .into_iter()
                .map(|s| JobStatusResponse {
                    name: s.name,
                    last_run: s.last_run.map(|dt| dt.to_string()),
                    last_status: s.last_status,
                    last_error: s.last_error,
                    run_count: s.run_count,
                    failure_count: s.failure_count,
                    avg_duration_ms: s.avg_duration_ms,
                })
                .collect();

            Ok(Json(response))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Get specific job status
pub async fn get_job_status(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(job_name): axum::extract::Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let monitor = JobMonitor::new(state.db.clone());

    match monitor.get_job_status(&job_name).await {
        Ok(Some(status)) => {
            let response = JobStatusResponse {
                name: status.name,
                last_run: status.last_run.map(|dt| dt.to_string()),
                last_status: status.last_status,
                last_error: status.last_error,
                run_count: status.run_count,
                failure_count: status.failure_count,
                avg_duration_ms: status.avg_duration_ms,
            };

            Ok(Json(response))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
