use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use std::sync::Arc;

use crate::db::connection::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/db/replicas", get(replica_stats))
}

async fn replica_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match &state.replicas {
        Some(mgr) => {
            let stats = mgr.stats().await;
            (StatusCode::OK, Json(serde_json::json!(stats)))
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!({ "total": 0, "healthy": 0, "replicas": [] })),
        ),
    }
}
