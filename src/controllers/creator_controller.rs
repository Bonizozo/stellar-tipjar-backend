use anyhow::Result;
use std::time::Instant;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::models::creator::{CreateCreatorRequest, Creator};

#[tracing::instrument(skip(state), fields(username = %req.username))]
pub async fn create_creator(state: &AppState, req: CreateCreatorRequest) -> Result<Creator> {
    let query = r#"
        INSERT INTO creators (id, username, wallet_address, created_at)
        VALUES ($1, $2, $3, NOW())
        RETURNING id, username, wallet_address, created_at
        "#;

    let start = Instant::now();
    let creator = sqlx::query_as::<_, Creator>(query)
        .bind(Uuid::new_v4())
        .bind(&req.username)
        .bind(&req.wallet_address)
        .fetch_one(&state.db)
        .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::info!(duration_ms = duration.as_millis(), "Creator created");

    Ok(creator)
}

#[tracing::instrument(skip(state), fields(username = %username))]
pub async fn get_creator_by_username(state: &AppState, username: &str) -> Result<Option<Creator>> {
    let query = r#"
        SELECT id, username, wallet_address, created_at
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

    Ok(creator)
}
