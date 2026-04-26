use axum::{routing::get, Router};
use std::sync::Arc;

use crate::controllers::scheduler_controller;
use crate::db::connection::AppState;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/scheduler/jobs", get(scheduler_controller::get_all_jobs))
        .route(
            "/scheduler/jobs/:name",
            get(scheduler_controller::get_job_status),
        )
        .with_state(state)
}
