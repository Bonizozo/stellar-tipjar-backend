use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::gateway::config::GatewayConfig;
use crate::gateway::context::GatewayIdentity;
use crate::models::api_key::ApiKey;
use crate::services::auth_service;

/// Gateway authentication middleware.
///
/// Runs before every `/api/*` request and resolves the caller's identity:
///
/// 1. If the path is in the public-path list → `GatewayIdentity::Anonymous`.
/// 2. If `Authorization: Bearer <token>` is present → validate JWT.
/// 3. If `X-API-Key` + `X-API-Secret` headers are present → validate API key.
/// 4. Otherwise → 401 Unauthorized.
///
/// The resolved `GatewayIdentity` is injected into `Request::extensions()` so
/// downstream handlers and middleware can read it without re-validating.
pub async fn gateway_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_owned();
    let config = GatewayConfig::default();

    // ── Public paths bypass auth ──────────────────────────────────────────────
    if config.is_public(&path) {
        req.extensions_mut().insert(GatewayIdentity::Anonymous);
        return next.run(req).await;
    }

    // ── Try Bearer JWT ────────────────────────────────────────────────────────
    if let Some(token) = bearer_token(req.headers()) {
        match auth_service::validate_token(&token, "access") {
            Ok(claims) => {
                let identity = GatewayIdentity::Jwt {
                    subject: claims.sub.clone(),
                    role: claims.role.clone(),
                    claims,
                };
                tracing::debug!(
                    identity = %identity.display(),
                    path = %path,
                    "Gateway: JWT authenticated"
                );
                req.extensions_mut().insert(identity);
                return next.run(req).await;
            }
            Err(_) => {
                return AppError::unauthorized("Invalid or expired token").into_response();
            }
        }
    }

    // ── Try API Key ───────────────────────────────────────────────────────────
    let api_key_header = header_str(req.headers(), "x-api-key");
    let api_secret_header = header_str(req.headers(), "x-api-secret");

    if let (Some(key), Some(secret)) = (api_key_header, api_secret_header) {
        match ApiKey::verify(&state.db, key, secret).await {
            Ok(api_key) => {
                ApiKey::record_usage(&state.db, &api_key.key).await;
                let identity = GatewayIdentity::ApiKey {
                    key_id: api_key.id,
                    key: api_key.key.clone(),
                    name: api_key.name.clone(),
                    permissions: api_key.permissions.clone(),
                };
                tracing::debug!(
                    identity = %identity.display(),
                    path = %path,
                    "Gateway: API key authenticated"
                );
                req.extensions_mut().insert(identity);
                return next.run(req).await;
            }
            Err(_) => {
                return AppError::unauthorized("Invalid or revoked API key").into_response();
            }
        }
    }

    // ── No credentials → reject ───────────────────────────────────────────────
    AppError::unauthorized(
        "Authentication required. Provide a Bearer token or X-API-Key / X-API-Secret headers.",
    )
    .into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_owned())
}

fn header_str<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}
