use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::errors::AppError;
use crate::security::permissions::Role;
use crate::services::auth_service;

/// Axum middleware that validates a Bearer JWT in the Authorization header.
/// On success, injects `Claims` and the parsed `Role` into request extensions.
pub async fn require_auth(mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_owned());

    let Some(token) = token else {
        return AppError::unauthorized("Missing Authorization header").into_response();
    };

    match auth_service::validate_token(&token, "access") {
        Ok(claims) => {
            // Parse the role string from the JWT into the typed Role enum.
            // Unknown roles fall back to Guest (least privilege).
            let role = Role::try_from(claims.role.as_str()).unwrap_or(Role::Guest);
            req.extensions_mut().insert(role);
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(_) => AppError::unauthorized("Invalid or expired token").into_response(),
    }
}
