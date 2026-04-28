use crate::collaboration::crdt::{Operation, SessionRegistry};
use crate::collaboration::history::CollaborationHistory;
use crate::collaboration::presence::PresenceManager;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub struct CollabState {
    pub sessions: Arc<SessionRegistry>,
    pub presence: Arc<PresenceManager>,
    pub history: Arc<CollaborationHistory>,
}

/// POST /collab/:document_id/ops
async fn submit_op(
    State(state): State<Arc<CollabState>>,
    Path(document_id): Path<Uuid>,
    Json(op): Json<Operation>,
) -> impl IntoResponse {
    let session = state.sessions.get_or_create(document_id).await;
    let applied = session.submit(op).await;

    if let Err(e) = state.history.record(&applied).await {
        tracing::warn!(error = %e, "Failed to persist collaboration op");
    }

    (StatusCode::OK, Json(applied))
}

/// GET /collab/:document_id/state
async fn get_state(
    State(state): State<Arc<CollabState>>,
    Path(document_id): Path<Uuid>,
) -> impl IntoResponse {
    match state.sessions.get(&document_id).await {
        Some(session) => {
            let doc = session.document_state().await;
            Json(json!({
                "document_id": document_id,
                "content": doc.content,
                "version": doc.version,
                "last_modified": doc.last_modified,
            }))
            .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        )
            .into_response(),
    }
}

/// GET /collab/:document_id/ops?since=<version>
#[derive(Deserialize)]
struct SinceQuery {
    since: Option<u64>,
}

async fn get_ops(
    State(state): State<Arc<CollabState>>,
    Path(document_id): Path<Uuid>,
    Query(q): Query<SinceQuery>,
) -> impl IntoResponse {
    match state.sessions.get(&document_id).await {
        Some(session) => {
            let ops = session.operations_since(q.since.unwrap_or(0)).await;
            Json(ops).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        )
            .into_response(),
    }
}

/// POST /collab/:document_id/presence
#[derive(Deserialize)]
struct PresenceBody {
    user_id: Uuid,
    username: String,
    cursor_position: Option<usize>,
}

async fn update_presence(
    State(state): State<Arc<CollabState>>,
    Path(document_id): Path<Uuid>,
    Json(body): Json<PresenceBody>,
) -> impl IntoResponse {
    state
        .presence
        .heartbeat(document_id, body.user_id, body.username, body.cursor_position)
        .await;
    StatusCode::NO_CONTENT
}

/// GET /collab/:document_id/presence
async fn get_presence(
    State(state): State<Arc<CollabState>>,
    Path(document_id): Path<Uuid>,
) -> impl IntoResponse {
    let users = state.presence.active_users(&document_id).await;
    Json(json!({ "active_users": users }))
}

/// DELETE /collab/:document_id/presence/:user_id
async fn leave_presence(
    State(state): State<Arc<CollabState>>,
    Path((document_id, user_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    state.presence.leave(document_id, user_id).await;
    StatusCode::NO_CONTENT
}

/// GET /collab/:document_id/history
#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<i64>,
}

async fn get_history(
    State(state): State<Arc<CollabState>>,
    Path(document_id): Path<Uuid>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    match state
        .history
        .list(document_id, None, q.limit.unwrap_or(100))
        .await
    {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to fetch collaboration history");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response()
        }
    }
}

pub fn router(state: Arc<CollabState>) -> Router {
    Router::new()
        .route("/collab/:document_id/ops", post(submit_op))
        .route("/collab/:document_id/state", get(get_state))
        .route("/collab/:document_id/ops", get(get_ops))
        .route(
            "/collab/:document_id/presence",
            post(update_presence).get(get_presence),
        )
        .route(
            "/collab/:document_id/presence/:user_id",
            axum::routing::delete(leave_presence),
        )
        .route("/collab/:document_id/history", get(get_history))
        .with_state(state)
}
