use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::connection::AppState;

async fn deployment_status() -> Json<Value> {
    let slot = std::env::var("DEPLOYMENT_SLOT").unwrap_or_else(|_| "unknown".into());
    Json(json!({ "slot": slot }))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/deployment/status", get(deployment_status))
}
