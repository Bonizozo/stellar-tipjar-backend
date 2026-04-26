use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
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

/// GET /monitoring/shards
///
/// Returns per-shard statistics (row counts, sizes, pool state).
/// Returns 200 with `{"sharding": "disabled"}` when sharding is not configured.
pub async fn shard_stats(
    State((state, _monitor)): State<(Arc<AppState>, Arc<MonitoringService>)>,
) -> impl IntoResponse {
    let Some(ref sharding) = state.sharding else {
        return (StatusCode::OK, Json(json!({ "sharding": "disabled" })));
    };

    let stats = sharding.stats().await;
    (StatusCode::OK, Json(json!({ "shards": stats })))
}

/// GET /monitoring/shards/health
///
/// Full cluster health check — pings every shard and returns reachability,
/// latency, and row counts.
pub async fn shard_health(
    State((state, _monitor)): State<(Arc<AppState>, Arc<MonitoringService>)>,
) -> impl IntoResponse {
    let Some(ref sharding) = state.sharding else {
        return (StatusCode::OK, Json(json!({ "sharding": "disabled" })));
    };

    let health = sharding.health().await;
    let status = if health.offline_shards > 0 {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (status, Json(json!(health)))
}

/// GET /monitoring/shards/balance
///
/// Analyse the current shard distribution and return a rebalance report.
pub async fn shard_balance(
    State((state, _monitor)): State<(Arc<AppState>, Arc<MonitoringService>)>,
) -> impl IntoResponse {
    let Some(ref sharding) = state.sharding else {
        return (StatusCode::OK, Json(json!({ "sharding": "disabled" })));
    };

    let report = sharding.analyze_balance().await;
    (StatusCode::OK, Json(json!(report)))
}

pub fn router(state: Arc<AppState>, monitor: Arc<MonitoringService>) -> Router {
    Router::new()
        .route("/monitoring/dashboard", get(dashboard))
        .route("/monitoring/shards", get(shard_stats))
        .route("/monitoring/shards/health", get(shard_health))
        .route("/monitoring/shards/balance", get(shard_balance))
        .with_state((state, monitor))
}
