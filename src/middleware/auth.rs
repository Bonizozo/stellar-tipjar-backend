use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::services::auth_service;

/// Axum middleware that validates a Bearer JWT in the Authorization header.
/// On success, injects the `Claims` into request extensions for downstream handlers.
/// Reads `jwt_secret` from `AppState::config.jwt.secret` — no env reads.
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_owned());

    let Some(token) = token else {
        return AppError::unauthorized("Missing Authorization header").into_response();
    };

    match auth_service::validate_token(&token, "access", &state.config.jwt.secret) {
        Ok(claims) => {
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(_) => AppError::unauthorized("Invalid or expired token").into_response(),
    }
}
