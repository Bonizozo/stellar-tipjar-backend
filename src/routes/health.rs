use crate::db::connection::AppState;
use crate::services::circuit_breaker::CircuitState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum DepStatus {
    Ok,
    Degraded,
    Unreachable,
    NotConfigured,
}

#[derive(Serialize)]
struct DepDetail {
    status: DepStatus,
    latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    dependencies: Dependencies,
}

#[derive(Serialize)]
struct Dependencies {
    postgres: DepDetail,
    redis: DepDetail,
    stellar: DepDetail,
}

/// Liveness + dependency health — GET /health
///
/// Returns 200 when all dependencies are healthy, 207 when any non-critical
/// dependency is degraded, 503 when postgres (critical) is unreachable.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "All dependencies healthy"),
        (status = 207, description = "One or more non-critical dependencies degraded"),
        (status = 503, description = "Critical dependency (postgres) unreachable"),
    )
)]
pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (postgres, redis, stellar) = tokio::join!(
        check_postgres(&state),
        check_redis(&state),
        check_stellar(&state),
    );

    let postgres_ok = matches!(postgres.status, DepStatus::Ok);
    let all_ok = postgres_ok
        && matches!(redis.status, DepStatus::Ok | DepStatus::NotConfigured)
        && matches!(stellar.status, DepStatus::Ok);

    let (http_status, label) = if !postgres_ok {
        (StatusCode::SERVICE_UNAVAILABLE, "unhealthy")
    } else if all_ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::MULTI_STATUS, "degraded")
    };

    (
        http_status,
        Json(HealthResponse {
            status: label,
            dependencies: Dependencies { postgres, redis, stellar },
        }),
    )
        .into_response()
}

/// Readiness probe — GET /ready
///
/// Returns 200 only when postgres (the critical dependency) is reachable.
/// Load balancers should use this endpoint to gate traffic.
#[utoipa::path(
    get,
    path = "/ready",
    tag = "health",
    responses(
        (status = 200, description = "Service is ready to accept traffic"),
        (status = 503, description = "Service is not ready (postgres unreachable)"),
    )
)]
pub async fn readiness_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let postgres = check_postgres(&state).await;

    if matches!(postgres.status, DepStatus::Ok) {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ready" })),
        )
            .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "reason": postgres.error.as_deref().unwrap_or("postgres unreachable"),
            })),
        )
            .into_response()
    }
}

// ---------------------------------------------------------------------------
// Per-dependency checks, each capped at CHECK_TIMEOUT (2 s)
// ---------------------------------------------------------------------------

async fn check_postgres(state: &AppState) -> DepDetail {
    // Respect circuit breaker — avoid hammering a dead DB
    if state.db_circuit_breaker.state() == CircuitState::Open {
        return DepDetail {
            status: DepStatus::Unreachable,
            latency_ms: None,
            error: Some("circuit breaker open".into()),
            detail: None,
        };
    }

    let pool = &state.db;
    let t = Instant::now();

    match timeout(CHECK_TIMEOUT, sqlx::query("SELECT 1").execute(pool)).await {
        Ok(Ok(_)) => {
            state.db_circuit_breaker.record_success();
            DepDetail {
                status: DepStatus::Ok,
                latency_ms: Some(t.elapsed().as_millis() as u64),
                error: None,
                detail: Some(serde_json::json!({
                    "pool_size":  pool.size(),
                    "pool_idle":  pool.num_idle(),
                })),
            }
        }
        Ok(Err(e)) => {
            state.db_circuit_breaker.record_failure();
            tracing::error!(error = %e, "Postgres health check failed");
            DepDetail {
                status: DepStatus::Unreachable,
                latency_ms: Some(t.elapsed().as_millis() as u64),
                error: Some(e.to_string()),
                detail: None,
            }
        }
        Err(_) => {
            state.db_circuit_breaker.record_failure();
            tracing::error!("Postgres health check timed out after 2s");
            DepDetail {
                status: DepStatus::Unreachable,
                latency_ms: Some(CHECK_TIMEOUT.as_millis() as u64),
                error: Some("timed out after 2s".into()),
                detail: None,
            }
        }
    }
}

async fn check_redis(state: &AppState) -> DepDetail {
    let Some(redis) = &state.redis else {
        return DepDetail {
            status: DepStatus::NotConfigured,
            latency_ms: None,
            error: None,
            detail: None,
        };
    };

    let mut conn = redis.clone();
    let t = Instant::now();

    match timeout(
        CHECK_TIMEOUT,
        redis::cmd("PING").query_async::<String>(&mut conn),
    )
    .await
    {
        Ok(Ok(_)) => DepDetail {
            status: DepStatus::Ok,
            latency_ms: Some(t.elapsed().as_millis() as u64),
            error: None,
            detail: None,
        },
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Redis health check failed");
            DepDetail {
                status: DepStatus::Unreachable,
                latency_ms: Some(t.elapsed().as_millis() as u64),
                error: Some(e.to_string()),
                detail: None,
            }
        }
        Err(_) => {
            tracing::warn!("Redis health check timed out after 2s");
            DepDetail {
                status: DepStatus::Degraded,
                latency_ms: Some(CHECK_TIMEOUT.as_millis() as u64),
                error: Some("timed out after 2s".into()),
                detail: None,
            }
        }
    }
}

async fn check_stellar(state: &AppState) -> DepDetail {
    let horizon_base = if state.stellar.network == "mainnet" {
        "https://horizon.stellar.org"
    } else {
        "https://horizon-testnet.stellar.org"
    };

    let t = Instant::now();

    let req = reqwest::Client::new()
        .get(horizon_base)
        .timeout(CHECK_TIMEOUT)
        .send();

    match timeout(CHECK_TIMEOUT, req).await {
        Ok(Ok(resp)) if resp.status().is_success() || resp.status().as_u16() == 200 => {
            DepDetail {
                status: DepStatus::Ok,
                latency_ms: Some(t.elapsed().as_millis() as u64),
                error: None,
                detail: None,
            }
        }
        Ok(Ok(resp)) => {
            tracing::warn!(status = %resp.status(), "Stellar Horizon returned non-200");
            DepDetail {
                status: DepStatus::Degraded,
                latency_ms: Some(t.elapsed().as_millis() as u64),
                error: Some(format!("HTTP {}", resp.status())),
                detail: None,
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Stellar health check failed");
            DepDetail {
                status: DepStatus::Unreachable,
                latency_ms: Some(t.elapsed().as_millis() as u64),
                error: Some(e.to_string()),
                detail: None,
            }
        }
        Err(_) => {
            tracing::warn!("Stellar health check timed out after 2s");
            DepDetail {
                status: DepStatus::Degraded,
                latency_ms: Some(CHECK_TIMEOUT.as_millis() as u64),
                error: Some("timed out after 2s".into()),
                detail: None,
            }
        }
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
}
