use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::comment::{Comment, CreateCommentRequest};

/// Create a new comment (or reply) on a tip.
pub async fn create_comment(
    pool: &PgPool,
    tip_id: Uuid,
    req: CreateCommentRequest,
) -> AppResult<Comment> {
    let comment = sqlx::query_as::<_, Comment>(
        r#"
        INSERT INTO comments (id, tip_id, parent_id, author, body)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, tip_id, parent_id, author, body, is_flagged, created_at, updated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tip_id)
    .bind(req.parent_id)
    .bind(&req.author)
    .bind(&req.body)
    .fetch_one(pool)
    .await?;
    Ok(comment)
}

/// List top-level comments for a tip (parent_id IS NULL), ordered oldest-first.
pub async fn list_comments(pool: &PgPool, tip_id: Uuid) -> AppResult<Vec<Comment>> {
    let comments = sqlx::query_as::<_, Comment>(
        r#"
        SELECT id, tip_id, parent_id, author, body, is_flagged, created_at, updated_at
        FROM comments
        WHERE tip_id = $1 AND parent_id IS NULL AND is_flagged = FALSE
        ORDER BY created_at ASC
        "#,
    )
    .bind(tip_id)
    .fetch_all(pool)
    .await?;
    Ok(comments)
}

/// List replies to a specific comment.
pub async fn list_replies(pool: &PgPool, parent_id: Uuid) -> AppResult<Vec<Comment>> {
    let replies = sqlx::query_as::<_, Comment>(
        r#"
        SELECT id, tip_id, parent_id, author, body, is_flagged, created_at, updated_at
        FROM comments
        WHERE parent_id = $1 AND is_flagged = FALSE
        ORDER BY created_at ASC
        "#,
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;
    Ok(replies)
}

/// Delete a comment by ID (hard delete).
pub async fn delete_comment(pool: &PgPool, comment_id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM comments WHERE id = $1")
        .bind(comment_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Flag a comment for moderation review.
pub async fn flag_comment(pool: &PgPool, comment_id: Uuid) -> AppResult<Comment> {
    let comment = sqlx::query_as::<_, Comment>(
        r#"
        UPDATE comments SET is_flagged = TRUE, updated_at = NOW()
        WHERE id = $1
        RETURNING id, tip_id, parent_id, author, body, is_flagged, created_at, updated_at
        "#,
    )
    .bind(comment_id)
    .fetch_one(pool)
    .await?;
    Ok(comment)
}
