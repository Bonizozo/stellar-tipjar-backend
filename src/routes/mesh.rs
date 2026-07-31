use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::service_mesh::discovery::ServiceRegistry;
use crate::service_mesh::mesh_monitor::mesh_health;

/// GET /mesh/health
///
/// Returns a health snapshot for every service registered in the mesh,
/// including instance counts and healthy/unhealthy breakdown.
pub async fn health_handler(
    State((_, registry)): State<(Arc<AppState>, Arc<ServiceRegistry>)>,
) -> impl IntoResponse {
    let snapshot = mesh_health(&registry).await;
    (StatusCode::OK, Json(snapshot))
}

/// GET /mesh/canary
///
/// Returns the current canary traffic weight configured in the traffic router.
pub async fn canary_handler(
    State((_, registry)): State<(Arc<AppState>, Arc<ServiceRegistry>)>,
) -> impl IntoResponse {
    // Discover stable vs canary instances for the primary service.
    let stable = registry.discover_all("stellar-tipjar-backend").await;
    let canary = registry.discover_all("stellar-tipjar-backend-canary").await;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "stable_instances": stable.len(),
            "canary_instances": canary.len(),
        })),
    )
}

pub fn router(state: Arc<AppState>, registry: Arc<ServiceRegistry>) -> Router {
    Router::new()
        .route("/mesh/health", get(health_handler))
        .route("/mesh/canary", get(canary_handler))
        .with_state((state, registry))
}
