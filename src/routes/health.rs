use crate::db::connection::AppState;
use crate::db::health::check_db;
use crate::services::circuit_breaker::CircuitState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use std::sync::Arc;

pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cb_state = state.db_circuit_breaker.state();
    let cb_open = cb_state == CircuitState::Open;

    if cb_open {
        tracing::warn!("Health check: DB circuit breaker is OPEN");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "circuit_open" })),
        )
            .into_response();
    }

    match check_db(&state.db).await {
        Ok(_) => {
            state.db_circuit_breaker.record_success();
            let pool = &state.db;
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "db": "connected",
                    "pool": {
                        "size": pool.size(),
                        "idle": pool.num_idle(),
                    },
                    "circuit_breaker": format!("{:?}", cb_state),
                })),
            )
                .into_response()
        }
        Err(e) => {
            state.db_circuit_breaker.record_failure();
            tracing::error!("Database health check failed: {:?}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "degraded", "db": "unreachable" })),
            )
                .into_response()
        }
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health_check))
}
