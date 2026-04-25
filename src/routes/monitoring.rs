use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::services::monitoring_service::MonitoringService;

/// GET /monitoring/dashboard
///
/// Returns a snapshot of the Stellar transaction monitoring stats plus
/// current database pool and circuit-breaker state.
pub async fn dashboard(
    State((state, monitor)): State<(Arc<AppState>, Arc<MonitoringService>)>,
) -> impl IntoResponse {
    let snap = monitor.stats.snapshot();
    let pool = &state.db;
    let cb_state = state.db_circuit_breaker.state();

    (
        StatusCode::OK,
        Json(json!({
            "stellar_monitoring": {
                "transactions_checked":  snap.transactions_checked,
                "transactions_verified": snap.transactions_verified,
                "transactions_failed":   snap.transactions_failed,
                "network_errors":        snap.network_errors,
            },
            "database": {
                "pool_size": pool.size(),
                "pool_idle": pool.num_idle(),
                "circuit_breaker": format!("{:?}", cb_state),
            },
            "stellar_network": state.stellar.network,
        })),
    )
}

pub fn router(state: Arc<AppState>, monitor: Arc<MonitoringService>) -> Router {
    Router::new()
        .route("/monitoring/dashboard", get(dashboard))
        .with_state((state, monitor))
}
