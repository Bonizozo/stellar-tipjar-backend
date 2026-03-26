use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::errors::AppError;
use crate::models::auth::Claims;
use crate::security::permissions::Permission;

/// Middleware factory: requires the authenticated user to have a specific permission.
/// Must be used after `require_auth` (which injects `Claims` into extensions).
///
/// Usage:
/// ```
/// .layer(axum::middleware::from_fn_with_state(state, |s, req, next| {
///     require_permission(s, req, next, Permission::DeleteCreator)
/// }))
/// ```
pub async fn require_permission(
    permission: Permission,
    req: Request,
    next: Next,
) -> Response {
    // Claims are injected by the upstream `require_auth` middleware.
    let claims = req.extensions().get::<Claims>().cloned();

    let Some(claims) = claims else {
        return AppError::unauthorized("Authentication required").into_response();
    };

    // Resolve the role stored in the JWT subject extension, or fall back to DB lookup.
    // For now we read the role from a custom JWT claim stored in `kind` field.
    // A richer implementation would inject the Role via a prior middleware.
    let role = req
        .extensions()
        .get::<crate::security::permissions::Role>()
        .cloned()
        .unwrap_or(crate::security::permissions::Role::Supporter);

    if !permission.allowed_for(&role) {
        return AppError::forbidden("Insufficient permissions").into_response();
    }

    next.run(req).await
}
