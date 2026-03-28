use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::errors::AppError;

/// Axum middleware that validates the `X-Admin-Key` header against hashed
/// keys stored in the `admin_users` table.
pub async fn require_admin(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let key = req
        .headers()
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    let Some(raw_key) = key else {
        return AppError::unauthorized("Missing X-Admin-Key header").into_response();
    };

    // Use your helper function here to avoid the LowerHex trait error
    let hash = hash_api_key(&raw_key);

    let exists = match sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM admin_users WHERE api_key_hash = $1)",
    )
    .bind(&hash)
    .fetch_one(&state.db)
    .await
    {
        Ok(found) => found,
        Err(e) => return AppError::from(e).into_response(),
    };

    if !exists {
        return AppError::unauthorized("Invalid admin key").into_response();
    }

    next.run(req).await
}

/// SHA-256 hex hash of a raw API key — used when seeding admin users.
pub fn hash_api_key(raw: &str) -> String {
    Sha256::digest(raw.as_bytes())
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}
