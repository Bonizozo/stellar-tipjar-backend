use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::security::permissions::Role;
use crate::security::token_revocation;
use crate::services::auth_service;

/// Axum middleware that validates a Bearer JWT in the Authorization header.
/// On success, injects `Claims` and the parsed `Role` into request extensions.
///
/// After signature/claims validation, checks the jti denylist and the
/// user's token-version epoch in a single Redis round trip. When Redis is
/// reachable but the check itself fails, the request is rejected
/// (fail-closed) rather than let through. When Redis isn't configured at
/// all for this deployment, revocation enforcement is simply unavailable —
/// see `docs/SESSION_SECURITY.md` for why Redis is required in production.
pub async fn require_auth(State(state): State<Arc<AppState>>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_owned());

    let Some(token) = token else {
        return AppError::unauthorized("Missing Authorization header").into_response();
    };

    let claims = match auth_service::validate_token(&token, "access") {
        Ok(claims) => claims,
        Err(_) => return AppError::unauthorized("Invalid or expired token").into_response(),
    };

    if let Some(ref redis) = state.redis {
        if let Err(e) = token_revocation::check_not_revoked(redis, &claims.jti, &claims.sub, claims.tv).await {
            return e.into_response();
        }
    } else {
        tracing::warn!("Redis not configured — access-token revocation checks are disabled");
    }

    // Parse the role string from the JWT into the typed Role enum.
    // Unknown roles fall back to Guest (least privilege).
    let role = Role::try_from(claims.role.as_str()).unwrap_or(Role::Guest);
    req.extensions_mut().insert(role);
    req.extensions_mut().insert(claims);
    next.run(req).await
}
