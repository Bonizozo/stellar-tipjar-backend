use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::feature_flag::{CreateFlagRequest, FeatureFlag, UpdateFlagRequest};

pub async fn list_flags(db: &PgPool) -> AppResult<Vec<FeatureFlag>> {
    let flags = sqlx::query_as::<_, FeatureFlag>(
        "SELECT id, name, description, enabled, rollout_pct, targeting, created_at, updated_at
         FROM feature_flags ORDER BY name",
    )
    .fetch_all(db)
    .await?;
    Ok(flags)
}

pub async fn get_flag(db: &PgPool, name: &str) -> AppResult<Option<FeatureFlag>> {
    let flag = sqlx::query_as::<_, FeatureFlag>(
        "SELECT id, name, description, enabled, rollout_pct, targeting, created_at, updated_at
         FROM feature_flags WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(db)
    .await?;
    Ok(flag)
}

pub async fn create_flag(db: &PgPool, req: CreateFlagRequest) -> AppResult<FeatureFlag> {
    let flag = sqlx::query_as::<_, FeatureFlag>(
        "INSERT INTO feature_flags (id, name, description, enabled, rollout_pct, targeting)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, name, description, enabled, rollout_pct, targeting, created_at, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(&req.name)
    .bind(&req.description)
    .bind(req.enabled)
    .bind(req.rollout_pct)
    .bind(&req.targeting)
    .fetch_one(db)
    .await?;
    Ok(flag)
}

pub async fn update_flag(db: &PgPool, name: &str, req: UpdateFlagRequest) -> AppResult<Option<FeatureFlag>> {
    let flag = sqlx::query_as::<_, FeatureFlag>(
        "UPDATE feature_flags
         SET description  = COALESCE($1, description),
             enabled      = COALESCE($2, enabled),
             rollout_pct  = COALESCE($3, rollout_pct),
             targeting    = COALESCE($4, targeting),
             updated_at   = NOW()
         WHERE name = $5
         RETURNING id, name, description, enabled, rollout_pct, targeting, created_at, updated_at",
    )
    .bind(&req.description)
    .bind(req.enabled)
    .bind(req.rollout_pct)
    .bind(req.targeting.as_ref())
    .bind(name)
    .fetch_optional(db)
    .await?;
    Ok(flag)
}

pub async fn delete_flag(db: &PgPool, name: &str) -> AppResult<bool> {
    let result = sqlx::query("DELETE FROM feature_flags WHERE name = $1")
        .bind(name)
        .execute(db)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Evaluate whether a flag is active for a given username.
/// Rules: flag must be enabled AND (rollout_pct == 100 OR username is in targeting list
/// OR username hash falls within rollout_pct bucket).
pub async fn evaluate(db: &PgPool, flag_name: &str, username: &str) -> AppResult<bool> {
    let Some(flag) = get_flag(db, flag_name).await? else {
        return Ok(false);
    };
    if !flag.enabled {
        return Ok(false);
    }
    // Check explicit targeting list
    if let Some(targets) = flag.targeting.as_array() {
        if targets.iter().any(|t| t.as_str() == Some(username)) {
            return Ok(true);
        }
    }
    // Percentage rollout via stable hash bucket
    if flag.rollout_pct >= 100 {
        return Ok(true);
    }
    if flag.rollout_pct > 0 {
        let bucket = username_bucket(username);
        return Ok(bucket < flag.rollout_pct as u8);
    }
    Ok(false)
}

/// Maps a username to a stable 0-99 bucket using a simple hash.
fn username_bucket(username: &str) -> u8 {
    let hash: u32 = username.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    (hash % 100) as u8
}
