use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::db::connection::AppState;

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
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing X-Admin-Key header" })),
        )
            .into_response();
    };

    let hash = format!("{:x}", Sha256::digest(raw_key.as_bytes()));

    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM admin_users WHERE api_key_hash = $1)",
    )
    .bind(&hash)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);

    if !exists {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid admin key" })),
        )
            .into_response();
    }

    next.run(req).await
}

/// SHA-256 hex hash of a raw API key — used when seeding admin users.
pub fn hash_api_key(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}
