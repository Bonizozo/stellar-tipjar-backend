use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::errors::{AppError, ValidationError};
use crate::services::distributed_lock::LockGuard;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/locks/stats", get(stats))
        .route("/locks/:resource", get(is_locked))
        .route("/locks/:resource/acquire", post(acquire))
        .route("/locks/:resource/release", delete(release))
        .route("/locks/:resource/renew", put(renew))
}

#[derive(Deserialize)]
struct AcquireBody {
    ttl_ms: Option<u64>,
}

#[derive(Deserialize)]
struct RenewBody {
    token: String,
    ttl_ms: Option<u64>,
}

#[derive(Deserialize)]
struct ReleaseBody {
    token: String,
}

fn lock_svc(state: &AppState) -> Result<&crate::services::distributed_lock::DistributedLockService, AppError> {
    state.lock_service.as_deref().ok_or_else(|| {
        AppError::service_unavailable("Distributed locking requires Redis")
    })
}

async fn stats(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse, AppError> {
    let s = lock_svc(&state)?.stats().await;
    Ok((StatusCode::OK, Json(s)))
}

async fn is_locked(
    State(state): State<Arc<AppState>>,
    Path(resource): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let locked = lock_svc(&state)?.is_locked(&resource).await;
    Ok((StatusCode::OK, Json(serde_json::json!({ "resource": resource, "locked": locked }))))
}

async fn acquire(
    State(state): State<Arc<AppState>>,
    Path(resource): Path<String>,
    Json(body): Json<AcquireBody>,
) -> Result<impl IntoResponse, AppError> {
    let ttl_ms = body.ttl_ms.unwrap_or(30_000).clamp(100, 300_000);
    match lock_svc(&state)?.acquire(&resource, ttl_ms).await {
        Ok(guard) => Ok((StatusCode::OK, Json(serde_json::json!({
            "resource": guard.resource,
            "token": guard.token,
            "ttl_ms": guard.ttl_ms,
        }))).into_response()),
        Err(crate::services::distributed_lock::LockError::AlreadyHeld) => {
            Ok((StatusCode::CONFLICT, Json(serde_json::json!({ "error": "lock already held" }))).into_response())
        }
        Err(e) => Err(AppError::service_unavailable(e.to_string())),
    }
}

async fn release(
    State(state): State<Arc<AppState>>,
    Path(resource): Path<String>,
    Json(body): Json<ReleaseBody>,
) -> Result<impl IntoResponse, AppError> {
    if body.token.is_empty() {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "token is required".to_string(),
        }));
    }
    let guard = LockGuard { resource: resource.clone(), token: body.token, ttl_ms: 0 };
    match lock_svc(&state)?.release(&guard).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(crate::services::distributed_lock::LockError::NotOwner) => {
            Ok((StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": "not the lock owner" }))).into_response())
        }
        Err(e) => Err(AppError::service_unavailable(e.to_string())),
    }
}

async fn renew(
    State(state): State<Arc<AppState>>,
    Path(resource): Path<String>,
    Json(body): Json<RenewBody>,
) -> Result<impl IntoResponse, AppError> {
    let ttl_ms = body.ttl_ms.unwrap_or(30_000).clamp(100, 300_000);
    let guard = LockGuard { resource: resource.clone(), token: body.token, ttl_ms };
    match lock_svc(&state)?.renew(&guard, ttl_ms).await {
        Ok(()) => Ok((StatusCode::OK, Json(serde_json::json!({ "resource": resource, "ttl_ms": ttl_ms }))).into_response()),
        Err(crate::services::distributed_lock::LockError::NotOwner) => {
            Ok((StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": "not the lock owner" }))).into_response())
        }
        Err(e) => Err(AppError::service_unavailable(e.to_string())),
    }
}
