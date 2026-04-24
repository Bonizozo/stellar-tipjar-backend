use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use sqlx::PgPool;

use crate::crypto::signing::verify_signature;
use crate::db::connection::AppState;
use crate::models::api_key::ApiKey;

const TIMESTAMP_TOLERANCE_SECS: i64 = 300; // 5 minutes

pub async fn verify_request_signature(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = req.headers();

    let api_key = header_str(headers, "x-api-key").ok_or(StatusCode::UNAUTHORIZED)?;
    let signature = header_str(headers, "x-signature").ok_or(StatusCode::UNAUTHORIZED)?;
    let timestamp: i64 = header_str(headers, "x-timestamp")
        .and_then(|s| s.parse().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Reject stale / future timestamps
    let now = chrono::Utc::now().timestamp();
    if (now - timestamp).abs() > TIMESTAMP_TOLERANCE_SECS {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Fetch secret (also validates key is active)
    let secret = ApiKey::get_secret(&state.db, &api_key)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Buffer body for signature verification
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let payload = String::from_utf8_lossy(&bytes);

    if !verify_signature(&secret, &payload, timestamp, &signature) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Replay-attack prevention: nonce = "<api_key>:<timestamp>"
    let nonce = format!("{}:{}", api_key, timestamp);
    if !check_and_store_nonce(&state.db, &nonce).await {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}

fn header_str<'a>(
    headers: &'a axum::http::HeaderMap,
    name: &str,
) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// Returns `true` if the nonce is fresh (not seen before) and was stored.
async fn check_and_store_nonce(pool: &PgPool, nonce: &str) -> bool {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM nonces WHERE nonce = $1)",
    )
    .bind(nonce)
    .fetch_one(pool)
    .await
    .unwrap_or(true); // fail-safe: treat DB error as replay

    if exists {
        return false;
    }

    sqlx::query(
        "INSERT INTO nonces (nonce, expires_at) VALUES ($1, NOW() + INTERVAL '5 minutes')
         ON CONFLICT DO NOTHING",
    )
    .bind(nonce)
    .execute(pool)
    .await
    .is_ok()
}
