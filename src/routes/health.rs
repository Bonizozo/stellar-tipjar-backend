use crate::db::connection::AppState;
use crate::db::health::check_db;
use axum::{extract::State, http::StatusCode, routing::get, Router};
use std::sync::Arc;

pub async fn health_check(State(state): State<Arc<AppState>>) -> Result<StatusCode, StatusCode> {
    match check_db(&state.db).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => {
            tracing::error!("Database health check failed: {:?}", e);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health_check))
}
