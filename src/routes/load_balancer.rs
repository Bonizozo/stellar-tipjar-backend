use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::service_mesh::discovery::ServiceRegistry;
use crate::service_mesh::load_balancer::{refresh_health, LoadBalancer, LoadBalancingStrategy};

/// GET /lb/health — returns health of all registered instances.
pub async fn lb_health(
    State((_, registry)): State<(Arc<AppState>, Arc<ServiceRegistry>)>,
) -> impl IntoResponse {
    let mut instances = registry.discover_all("stellar-tipjar-backend").await;
    refresh_health(&mut instances).await;
    let healthy = instances.iter().filter(|i| i.healthy).count();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total": instances.len(),
            "healthy": healthy,
            "unhealthy": instances.len() - healthy,
            "instances": instances,
        })),
    )
}

/// GET /lb/failover — selects the next healthy instance (round-robin).
pub async fn lb_failover(
    State((_, registry)): State<(Arc<AppState>, Arc<ServiceRegistry>)>,
) -> impl IntoResponse {
    let instances = registry.discover_all("stellar-tipjar-backend").await;
    let lb = LoadBalancer::new(LoadBalancingStrategy::RoundRobin);
    match lb.select(&instances, None).await {
        Some(inst) => (StatusCode::OK, Json(serde_json::json!({ "selected": inst }))).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no healthy instances available" })),
        )
            .into_response(),
    }
}

/// DELETE /lb/sessions/:key — clears a sticky session binding.
pub async fn clear_sticky_session(
    State((_, registry)): State<(Arc<AppState>, Arc<ServiceRegistry>)>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let instances = registry.discover_all("stellar-tipjar-backend").await;
    let lb = LoadBalancer::new(LoadBalancingStrategy::RoundRobin);
    // Seed the session map by selecting once so the key exists, then clear it.
    lb.select(&instances, Some(&key)).await;
    lb.clear_session(&key).await;
    StatusCode::NO_CONTENT
}

pub fn router(state: Arc<AppState>, registry: Arc<ServiceRegistry>) -> Router {
    Router::new()
        .route("/lb/health", get(lb_health))
        .route("/lb/failover", get(lb_failover))
        .route("/lb/sessions/:key", delete(clear_sticky_session))
        .with_state((state, registry))
}
