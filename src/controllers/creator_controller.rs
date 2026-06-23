use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{PublicKey, Signature, Verifier};
use std::time::Instant;
use uuid::Uuid;

use crate::cache::{keys, redis_client};
use crate::db::connection::AppState;
use crate::db::query_logger::QueryLogger;
use crate::errors::{AppError, AppResult, ValidationError};
use crate::metrics::collectors::CREATORS_REGISTERED_TOTAL;
use crate::models::creator::{CreateCreatorRequest, Creator, UpdateCreatorProfileRequest, UpdateCreatorWalletRequest};
use crate::moderation::ContentType;
use crate::search::SearchQuery;
use sqlx::PgPool;
use validator::Validate;

#[tracing::instrument(skip(state), fields(username = %req.username))]
pub async fn create_creator(state: &AppState, req: CreateCreatorRequest) -> AppResult<Creator> {
    // Moderate the requested username before persisting.
    let moderation = state
        .moderation
        .check_content(&req.username, ContentType::Username, None)
        .await;
    if moderation.has_high_confidence_violation(0.90) {
        return Err(AppError::Validation(ValidationError::InvalidRequest {
            message: "Username was rejected by content moderation".to_string(),
        }));
    }

    let query = r#"
        INSERT INTO creators (id, username, wallet_address, email, created_at, bio, display_name, avatar_url, social_links, categories, tags)
        VALUES ($1, $2, $3, $4, NOW(), $5, $6, $7, $8::jsonb, $9, $10)
        RETURNING id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at,
                  bio, display_name, avatar_url, is_verified, social_links, categories, tags
        "#;

    let start = Instant::now();
    let social_json = req.social_links.as_ref().map(|links| {
        serde_json::to_value(links).unwrap_or(serde_json::Value::Array(vec![]))
    });
    let creator = sqlx::query_as::<_, Creator>(query)
        .bind(Uuid::new_v4())
        .bind(&req.username)
        .bind(&req.wallet_address)
        .bind(&req.email)
        .bind(&req.bio)
        .bind(&req.display_name)
        .bind(&req.avatar_url)
        .bind(&social_json)
        .bind(req.categories.as_ref().map(|c| c.as_slice()))
        .bind(req.tags.as_ref().map(|t| t.as_slice()))
        .fetch_one(&state.db)
        .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::info!(duration_ms = duration.as_millis(), "Creator created");
    CREATORS_REGISTERED_TOTAL.inc();

    // Cache the new creator and invalidate any stale search results.
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let _ = redis_client::set(
            &mut conn,
            &keys::creator(&creator.username),
            &creator,
            redis_client::TTL_CREATOR,
        )
        .await;
    }

    // Centralized invalidation for search and creator list caches
    if let Some(ref inv) = state.invalidator {
        let _ = inv.invalidate_pattern("search:creators:*").await;
        let _ = inv
            .invalidate_pattern(&keys::http_response_pattern("/creators/"))
            .await;
    }

    // Main branch added Webhook notification
    crate::webhooks::trigger_webhooks(
        state.db.clone(),
        "creator.created",
        serde_json::to_value(&creator).unwrap(),
    )
    .await;
    // Notify external services via webhook.
    let payload = serde_json::to_value(&creator).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize creator webhook payload");
        AppError::internal()
    })?;
    crate::webhooks::trigger_webhooks(state.db.clone(), "creator.created", payload).await;

    // Audit log: creator created
    {
        let db = state.db.clone();
        let username = creator.username.clone();
        let creator_id = creator.id.to_string();
        tokio::spawn(async move {
            let _ = crate::controllers::audit_log_controller::log(
                &db,
                "creator.created",
                Some(&username),
                "creator",
                Some(&creator_id),
                "create",
                None,
                None,
                serde_json::json!({}),
                None,
                None,
            )
            .await;
        });
    }

    Ok(creator)
}

#[tracing::instrument(skip(state), fields(username = %username))]
pub async fn get_creator_by_username(
    state: &AppState,
    username: &str,
) -> AppResult<Option<Creator>> {
    let query = r#"
        SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at, bio, display_name, avatar_url, is_verified, social_links, categories, tags
        FROM creators
        WHERE username = $1
        "#;

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        if let Some(cached) =
            redis_client::get::<Creator>(&mut conn, &keys::creator(username)).await
        {
            return Ok(Some(cached));
        }
    }

    let start = Instant::now();
    let creator = sqlx::query_as::<_, Creator>(query)
        .bind(username)
        .fetch_optional(&state.db)
        .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::debug!(
        duration_ms = duration.as_millis(),
        found = creator.is_some(),
        "Creator lookup"
    );

    // Populate cache if found.
    if let (Some(ref c), Some(conn)) = (&creator, state.redis.as_ref()) {
        let mut conn = conn.clone();
        let _ = redis_client::set(
            &mut conn,
            &keys::creator(username),
            c,
            redis_client::TTL_CREATOR,
        )
        .await;
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
pub async fn update_creator_wallet_address(
    state: &AppState,
    username: &str,
    req: UpdateCreatorWalletRequest,
) -> AppResult<Creator> {
    req.validate().map_err(|e| AppError::Validation(ValidationError::InvalidRequest {
        message: e.to_string(),
    }))?;

    let existing_creator = get_creator_or_not_found(state, username).await?;
    verify_wallet_signature(&req.new_wallet_address, &req.signature, req.new_wallet_address.as_bytes())?;

    let query = r#"
        UPDATE creators
        SET wallet_address = $1
        WHERE username = $2
        RETURNING id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at,
                  bio, display_name, avatar_url, is_verified, social_links, categories, tags
        "#;

    let start = Instant::now();
    let creator = sqlx::query_as::<_, Creator>(query)
        .bind(&req.new_wallet_address)
        .bind(username)
        .fetch_one(&state.db)
        .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::info!(duration_ms = duration.as_millis(), username = %username, "Creator wallet address updated");

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let _ = redis_client::set(
            &mut conn,
            &keys::creator(&creator.username),
            &creator,
            redis_client::TTL_CREATOR,
        )
        .await;
    }

    if let Some(ref inv) = state.invalidator {
        let _ = inv.invalidate_pattern("search:creators:*").await;
        let _ = inv
            .invalidate_pattern(&keys::http_response_pattern("/creators/"))
            .await;
    }

    {
        let db = state.db.clone();
        let username = creator.username.clone();
        let creator_id = creator.id.to_string();
        let before_data = serde_json::json!({ "wallet_address": existing_creator.wallet_address });
        let after_data = serde_json::json!({ "wallet_address": creator.wallet_address });
        tokio::spawn(async move {
            let _ = crate::controllers::audit_log_controller::log(
                &db,
                "creator.wallet_address.updated",
                Some(&username),
                "creator",
                Some(&creator_id),
                "update",
                Some(before_data),
                Some(after_data),
                serde_json::json!({}),
                None,
                None,
            )
            .await;
        });
    }

    Ok(creator)
}

/// Update a creator's profile fields (bio, display_name, avatar_url, social_links, categories, tags).
/// Only provided (non-null) fields are updated.
#[tracing::instrument(skip(state), fields(username = %username))]
pub async fn update_creator_profile(
    state: &AppState,
    username: &str,
    req: UpdateCreatorProfileRequest,
) -> AppResult<Creator> {
    req.validate().map_err(|e| AppError::Validation(ValidationError::InvalidRequest {
        message: e.to_string(),
    }))?;

    let existing = get_creator_or_not_found(state, username).await?;

    let query = r#"
        UPDATE creators
        SET
            bio = COALESCE($1, bio),
            display_name = COALESCE($2, display_name),
            avatar_url = COALESCE($3, avatar_url),
            social_links = COALESCE($4::jsonb, social_links),
            categories = COALESCE($5, categories),
            tags = COALESCE($6, tags)
        WHERE username = $7
        RETURNING id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at,
                  bio, display_name, avatar_url, is_verified, social_links, categories, tags
        "#;

    let start = Instant::now();
    let social_json = req.social_links.as_ref().map(|links| {
        serde_json::to_value(links).unwrap_or(serde_json::Value::Array(vec![]))
    });
    let creator = sqlx::query_as::<_, Creator>(query)
        .bind(&req.bio)
        .bind(&req.display_name)
        .bind(&req.avatar_url)
        .bind(&social_json)
        .bind(req.categories.as_ref().map(|c| c.as_slice()))
        .bind(req.tags.as_ref().map(|t| t.as_slice()))
        .bind(username)
        .fetch_one(&state.db)
        .await?;
    let duration = start.elapsed();

    QueryLogger::log_query(query, duration);
    state.performance.track_query(query, duration);
    tracing::info!(duration_ms = duration.as_millis(), username = %username, "Creator profile updated");

    // Update cache
    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        let _ = redis_client::set(
            &mut conn,
            &keys::creator(&creator.username),
            &creator,
            redis_client::TTL_CREATOR,
        )
        .await;
    }

    if let Some(ref inv) = state.invalidator {
        let _ = inv.invalidate_pattern("search:creators:*").await;
        let _ = inv
            .invalidate_pattern(&keys::http_response_pattern("/creators/"))
            .await;
    }

    // Audit log
    {
        let db = state.db.clone();
        let uname = creator.username.clone();
        let creator_id = creator.id.to_string();
        let before_data = serde_json::json!({
            "bio": existing.bio,
            "display_name": existing.display_name,
            "avatar_url": existing.avatar_url,
            "social_links": existing.social_links,
            "categories": existing.categories,
            "tags": existing.tags,
        });
        let after_data = serde_json::json!({
            "bio": creator.bio,
            "display_name": creator.display_name,
            "avatar_url": creator.avatar_url,
            "social_links": creator.social_links,
            "categories": creator.categories,
            "tags": creator.tags,
        });
        tokio::spawn(async move {
            let _ = crate::controllers::audit_log_controller::log(
                &db,
                "creator.profile.updated",
                Some(&uname),
                "creator",
                Some(&creator_id),
                "update",
                Some(before_data),
                Some(after_data),
                serde_json::json!({}),
                None,
                None,
            )
            .await;
        });
    }

    Ok(creator)
}

fn verify_wallet_signature(
    public_key: &str,
    signature: &str,
    message: &[u8],
) -> AppResult<()> {
    let public_key_bytes = crate::validation::stellar::decode_stellar_public_key(public_key)
        .map_err(|_| AppError::bad_request("Invalid Stellar public key"))?;
    let public_key = PublicKey::from_bytes(&public_key_bytes)
        .map_err(|_| AppError::bad_request("Invalid Stellar public key"))?;
    let signature_bytes = general_purpose::STANDARD
        .decode(signature)
        .map_err(|_| AppError::bad_request("Invalid signature encoding"))?;
    let signature = Signature::from_bytes(&signature_bytes)
        .map_err(|_| AppError::bad_request("Invalid signature"))?;
    public_key
        .verify(message, &signature)
        .map_err(|_| AppError::bad_request("Wallet signature verification failed"))?;
    Ok(())
}

#[tracing::instrument(skip(state), fields(username = %username))]
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
        SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at, bio, display_name, avatar_url, is_verified, social_links, categories, tags
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
