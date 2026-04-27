use crate::db::connection::AppState;
use crate::db::health::check_db;
use crate::services::circuit_breaker::CircuitState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use std::sync::Arc;

/// Liveness probe — checks DB connectivity and circuit-breaker state
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy",
         example = json!({ "status": "ok", "db": "connected" })),
        (status = 503, description = "Service degraded",
         example = json!({ "status": "degraded", "db": "circuit_open" }))
    )
)]
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

/// Readiness probe — checks DB, Stellar network, and Redis
#[utoipa::path(
    get,
    path = "/ready",
    tag = "health",
    responses(
        (status = 200, description = "All dependencies reachable",
         example = json!({ "status": "ready", "checks": { "db": "ok", "stellar": "ok", "redis": "ok" } })),
        (status = 503, description = "One or more dependencies unreachable",
         example = json!({ "status": "not_ready", "checks": { "db": "unreachable", "stellar": "ok", "redis": "not_configured" } }))
    )
)]
pub async fn readiness_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut all_ok = true;
    let mut checks = serde_json::Map::new();

    // --- Database ---
    match check_db(&state.db).await {
        Ok(_) => {
            checks.insert("db".into(), json!("ok"));
        }
        Err(e) => {
            tracing::error!("Readiness: DB check failed: {:?}", e);
            checks.insert("db".into(), json!("unreachable"));
            all_ok = false;
        }
    }

    // --- Stellar network (Horizon ping) ---
    let horizon_base = if state.stellar.network == "mainnet" {
        "https://horizon.stellar.org"
    } else {
        "https://horizon-testnet.stellar.org"
    };
    match reqwest::Client::new()
        .get(horizon_base)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 200 => {
            checks.insert("stellar".into(), json!("ok"));
        }
        Ok(resp) => {
            tracing::warn!("Readiness: Stellar returned {}", resp.status());
            checks.insert("stellar".into(), json!(format!("degraded ({})", resp.status())));
        }
        Err(e) => {
            tracing::error!("Readiness: Stellar check failed: {:?}", e);
            checks.insert("stellar".into(), json!("unreachable"));
            all_ok = false;
        }
    }

    // --- Redis ---
    match &state.redis {
        Some(redis) => {
            let mut conn = redis.clone();
            match redis::cmd("PING")
                .query_async::<String>(&mut conn)
                .await
            {
                Ok(_) => {
                    checks.insert("redis".into(), json!("ok"));
                }
                Err(e) => {
                    tracing::error!("Readiness: Redis check failed: {:?}", e);
                    checks.insert("redis".into(), json!("unreachable"));
                    all_ok = false;
                }
            }
        }
        None => {
            checks.insert("redis".into(), json!("not_configured"));
        }
    }

    let status = if all_ok { "ready" } else { "not_ready" };
    let code = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (code, Json(json!({ "status": status, "checks": checks }))).into_response()
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
}
