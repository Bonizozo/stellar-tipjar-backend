use std::time::Instant;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::errors::{AppError, AppResult, ValidationError};
use crate::models::creator::{CreateCreatorRequest, Creator};
use crate::search::SearchQuery;
use crate::cache::{redis_client, keys};
use sqlx::PgPool;

#[tracing::instrument(skip(state), fields(username = %req.username))]
pub async fn create_creator(state: &AppState, req: CreateCreatorRequest) -> AppResult<Creator> {
    let query = r#"
        INSERT INTO creators (id, username, wallet_address, email, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        RETURNING id, username, wallet_address, email, created_at
        "#;

    let start = Instant::now();
    let creator = sqlx::query_as::<_, Creator>(query)
    .bind(Uuid::new_v4())
    .bind(&req.username)
    .bind(&req.wallet_address)
    .bind(&req.email)
    .fetch_one(&state.db)
    .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::info!(duration_ms = duration.as_millis(), "Creator created");

    // Warm the cache immediately after creation.
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let _ = redis_client::set(&mut conn, &keys::creator(&creator.username), &creator, redis_client::TTL_CREATOR).await;
    }

    // Notify external services via webhook.
    let payload = serde_json::to_value(&creator).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize creator webhook payload");
        AppError::internal()
    })?;
    crate::webhooks::trigger_webhooks(state.db.clone(), "creator.created", payload).await;

    Ok(creator)
}

#[tracing::instrument(skip(state), fields(username = %username))]
pub async fn get_creator_by_username(state: &AppState, username: &str) -> AppResult<Option<Creator>> {
    let query = r#"
        SELECT id, username, wallet_address, email, created_at
        FROM creators
        WHERE username = $1
        "#;

    let start = Instant::now();
    let creator = sqlx::query_as::<_, Creator>(query)
        .bind(username)
        .fetch_optional(&state.db)
        .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::debug!(duration_ms = duration.as_millis(), found = creator.is_some(), "Creator lookup");

    // Populate cache if found.
    if let (Some(ref c), Some(conn)) = (&creator, state.redis.as_ref()) {
        let mut conn = conn.clone();
        let _ = redis_client::set(&mut conn, &keys::creator(username), c, redis_client::TTL_CREATOR).await;
    }

    Ok(creator)
}

#[tracing::instrument(skip(state), fields(username = %username))]
pub async fn get_creator_or_not_found(state: &AppState, username: &str) -> AppResult<Creator> {
    let creator = get_creator_by_username(state, username).await?;
    creator.ok_or_else(|| AppError::CreatorNotFound {
        username: username.to_string(),
    })
}

/// Search creators by username using PostgreSQL full-text search with trigram
/// fuzzy fallback. Results are ranked by ts_rank descending.
pub async fn search_creators(pool: &PgPool, query: &SearchQuery) -> AppResult<Vec<Creator>> {
    let term = query.q.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "Query parameter 'q' must not be empty".to_string(),
        }));
    }
    let limit = query.clamped_limit();

    let creators = sqlx::query_as::<_, Creator>(
        r#"
        SELECT id, username, wallet_address, email, created_at
        FROM creators
        WHERE
            search_vector @@ plainto_tsquery('english', $1)
            OR username ILIKE '%' || $1 || '%'
        ORDER BY
            ts_rank(search_vector, plainto_tsquery('english', $1)) DESC,
            created_at DESC
        LIMIT $2
        "#,
    )
    .bind(&term)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(creators)
}





