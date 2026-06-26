use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::{comment_controller, notification_controller};
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::comment::{CommentResponse, CreateCommentRequest};
use crate::moderation::ContentType;
use crate::validation::ValidatedJson;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tips/:tip_id/comments", post(create_comment).get(list_comments))
        .route("/tips/:tip_id/comments/:id/replies", get(list_replies))
        .route("/tips/:tip_id/comments/:id", delete(delete_comment))
        .route("/tips/:tip_id/comments/:id/flag", post(flag_comment))
}

async fn create_comment(
    State(state): State<Arc<AppState>>,
    Path(tip_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<CreateCommentRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Moderate comment body before persisting.
    let moderation = state
        .moderation
        .check_content(&body.body, ContentType::TipMessage, None)
        .await;
    if moderation.has_high_confidence_violation(0.90) {
        return Err(AppError::bad_request("Comment was rejected by content moderation"));
    }

    let comment = comment_controller::create_comment(&state.db, tip_id, body).await?;

    // Notify the tip's creator that a new comment was posted.
    // Look up the creator username from the tip; ignore errors so the comment still succeeds.
    if let Ok(row) = sqlx::query_scalar::<_, String>(
        "SELECT creator_username FROM tips WHERE id = $1",
    )
    .bind(tip_id)
    .fetch_one(&state.db)
    .await
    {
        let _ = notification_controller::create_notification(
            &state.db,
            &row,
            "comment_received",
            serde_json::json!({
                "tip_id": tip_id,
                "comment_id": comment.id,
                "author": comment.author,
            }),
        )
        .await;
    }

    Ok((StatusCode::CREATED, Json(CommentResponse::from(comment))))
}

async fn list_comments(
    State(state): State<Arc<AppState>>,
    Path(tip_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let comments = comment_controller::list_comments(&state.db, tip_id).await?;
    let response: Vec<CommentResponse> = comments.into_iter().map(CommentResponse::from).collect();
    Ok((StatusCode::OK, Json(response)))
}

async fn list_replies(
    State(state): State<Arc<AppState>>,
    Path((_tip_id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let replies = comment_controller::list_replies(&state.db, comment_id).await?;
    let response: Vec<CommentResponse> = replies.into_iter().map(CommentResponse::from).collect();
    Ok((StatusCode::OK, Json(response)))
}

async fn delete_comment(
    State(state): State<Arc<AppState>>,
    Path((_tip_id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    comment_controller::delete_comment(&state.db, comment_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn flag_comment(
    State(state): State<Arc<AppState>>,
    Path((_tip_id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let comment = comment_controller::flag_comment(&state.db, comment_id).await?;
    Ok((StatusCode::OK, Json(CommentResponse::from(comment))))
}
