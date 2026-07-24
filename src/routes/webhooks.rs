use crate::db::connection::AppState;
use crate::webhooks::{
    self, CreateWebhookRequest, UpdateWebhookRequest,
};
use crate::webhooks::retry::{list_dlq, replay_dlq_entry, WebhookRetryConfig};
use crate::webhooks::sender::DeliveryContext;
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

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/webhooks", get(list).post(create))
        .route("/webhooks/:id", get(get_one).put(update).delete(remove))
        .route("/webhooks/:id/logs", get(delivery_logs))
        .route("/webhooks/:id/test", post(test_webhook))
        .route("/webhooks/:id/rotate-secret", post(rotate_secret))
        .route("/webhooks/dlq", get(dlq_list))
        .route("/webhooks/dlq/:id/replay", post(dlq_replay))
        .with_state(state)
}

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
/// Sends a test ping to the webhook URL using the current secrets.
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response();
        }
    };

    let payload = json!({
        "id": uuid::Uuid::new_v4(),
        "event_type": "webhook.test",
        "payload": { "message": "This is a test event from stellar-tipjar" },
        "timestamp": chrono::Utc::now()
    });

    // Use dual secrets if a rotation is in progress.
    let secrets = webhooks::active_secrets(&state.db, id)
        .await
        .unwrap_or_else(|_| vec![hook.secret.clone()]);

    let mut ctx = DeliveryContext::new("webhook.test", secrets[0].clone());
    if secrets.len() > 1 {
        ctx.secrets = secrets;
    }

    match webhooks::sender::send_webhook(&hook.url, &ctx, payload.clone()).await {
        Ok(status) => {
            let _ = webhooks::log_delivery(
                &state.db,
                id,
                "webhook.test",
                &payload,
                Some(status as i32),
                None,
                true,
                1,
            )
            .await;
            Json(json!({
                "status": "delivered",
                "delivery_id": ctx.delivery_id,
                "http_status": status
            }))
            .into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let _ = webhooks::log_delivery(
                &state.db,
                id,
                "webhook.test",
                &payload,
                None,
                Some(&msg),
                false,
                1,
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

/// POST /webhooks/:id/rotate-secret
///
/// Rotates the signing secret for the webhook.  The retiring secret is kept
/// in `webhook_secrets` for the overlap window so existing receivers continue
/// to verify deliveries.  The rotation is recorded in the audit log.
///
/// Response:
/// ```json
/// { "new_secret": "...", "retiring_secret": "..." }
/// ```
/// The caller must distribute `new_secret` to receivers; they have the
/// tolerance window to update before the retiring secret expires.
async fn rotate_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Verify the webhook exists before rotating.
    match webhooks::get_webhook(&state.db, id).await {
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response()
        }
        Err(e) => {
            tracing::error!("rotate_secret get: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response();
        }
        Ok(Some(_)) => {}
    }

    match webhooks::rotate_secret(&state.db, id, 2).await {
        Ok((new_secret, retiring)) => {
            // Audit log — fire-and-forget.
            {
                let db = state.db.clone();
                let wid = id.to_string();
                tokio::spawn(async move {
                    let _ = crate::controllers::audit_log_controller::log(
                        &db,
                        "webhook.secret_rotated",
                        None,
                        "webhook",
                        Some(&wid),
                        "rotate_secret",
                        None,
                        Some(serde_json::json!({ "webhook_id": wid })),
                        serde_json::json!({}),
                        None,
                        None,
                    )
                    .await;
                });
            }

            tracing::info!(webhook_id = %id, "Webhook secret rotated");

            Json(json!({
                "new_secret":      new_secret,
                "retiring_secret": retiring,
                "note": "Distribute new_secret to receivers. Both secrets are valid \
                         during the rotation overlap window."
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("rotate_secret: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "rotation failed"})),
            )
                .into_response()
        }
    }
}

/// GET /webhooks/dlq
async fn dlq_list(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match list_dlq(&state.db, 50).await {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => {
            tracing::error!("list_dlq: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}

/// POST /webhooks/dlq/:id/replay
async fn dlq_replay(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let config = WebhookRetryConfig::default();
    match replay_dlq_entry(&state.db, id, &config).await {
        Ok(status) => Json(status).into_response(),
        Err(sqlx::Error::RowNotFound) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response()
        }
        Err(e) => {
            tracing::error!("dlq_replay: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response()
        }
    }
}
