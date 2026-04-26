use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use std::sync::Arc;

use crate::db::connection::AppState;

/// GET /profiling/dashboard
///
/// Returns aggregated query stats: call count, average latency (ms), and max latency (ms)
/// for every distinct query pattern seen since startup.
pub async fn dashboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.performance.get_stats();

    let mut queries: Vec<_> = stats
        .into_iter()
        .map(|(pattern, (count, avg_ms, max_ms))| {
            json!({
                "query": pattern,
                "count": count,
                "avg_ms": (avg_ms * 100.0).round() / 100.0,
                "max_ms": max_ms,
                "slow": max_ms > 200,
            })
        })
        .collect();

    // Sort slowest first so the dashboard highlights problem queries at the top.
    queries.sort_by(|a, b| {
        b["max_ms"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["max_ms"].as_u64().unwrap_or(0))
    });

    (StatusCode::OK, Json(json!({ "queries": queries })))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/profiling/dashboard", get(dashboard))
        .with_state(state)
}
