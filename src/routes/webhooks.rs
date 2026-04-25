use crate::db::connection::AppState;
use crate::webhooks::{
    self, CreateWebhookRequest, UpdateWebhookRequest,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// GET /webhooks
async fn list(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match webhooks::list_webhooks(&state.db).await {
        Ok(hooks) => Json(hooks).into_response(),
        Err(e) => {
            tracing::error!("list_webhooks: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// POST /webhooks
async fn create(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateWebhookRequest>,
) -> impl IntoResponse {
    match webhooks::create_webhook(&state.db, body).await {
        Ok(hook) => (StatusCode::CREATED, Json(hook)).into_response(),
        Err(e) => {
            tracing::error!("create_webhook: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// GET /webhooks/:id
async fn get_one(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match webhooks::get_webhook(&state.db, id).await {
        Ok(Some(hook)) => Json(hook).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response(),
        Err(e) => {
            tracing::error!("get_webhook: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// PUT /webhooks/:id
async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateWebhookRequest>,
) -> impl IntoResponse {
    match webhooks::update_webhook(&state.db, id, body).await {
        Ok(Some(hook)) => Json(hook).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response(),
        Err(e) => {
            tracing::error!("update_webhook: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// DELETE /webhooks/:id
async fn remove(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match webhooks::delete_webhook(&state.db, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response(),
        Err(e) => {
            tracing::error!("delete_webhook: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// GET /webhooks/:id/logs
async fn delivery_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match webhooks::list_delivery_logs(&state.db, id, 50).await {
        Ok(logs) => Json(logs).into_response(),
        Err(e) => {
            tracing::error!("list_delivery_logs: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// POST /webhooks/:id/test
/// Sends a test ping event to the webhook URL immediately (no retry).
async fn test_webhook(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let hook = match webhooks::get_webhook(&state.db, id).await {
        Ok(Some(h)) => h,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response()
        }
        Err(e) => {
            tracing::error!("test_webhook get: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response();
        }
    };

    let payload = json!({
        "id": uuid::Uuid::new_v4(),
        "event_type": "webhook.test",
        "payload": { "message": "This is a test event from stellar-tipjar" },
        "timestamp": chrono::Utc::now()
    });

    match webhooks::sender::send_webhook(&hook.url, &hook.secret, payload.clone()).await {
        Ok(_) => {
            let _ = webhooks::log_delivery(
                &state.db, id, "webhook.test", &payload, Some(200), None, true, 1,
            )
            .await;
            Json(json!({"status": "delivered"})).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let _ = webhooks::log_delivery(
                &state.db, id, "webhook.test", &payload, None, Some(&msg), false, 1,
            )
            .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"status": "failed", "error": msg})),
            )
                .into_response()
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/webhooks", get(list).post(create))
        .route("/webhooks/:id", get(get_one).put(update).delete(remove))
        .route("/webhooks/:id/logs", get(delivery_logs))
        .route("/webhooks/:id/test", post(test_webhook))
        .with_state(state)
}
