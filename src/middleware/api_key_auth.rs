use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::api_key::ApiKey;

/// Axum middleware that validates `X-API-Key` + `X-API-Secret` headers.
/// Injects the verified `ApiKey` into request extensions and increments usage.
pub async fn require_api_key(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let key = header_str(req.headers(), "x-api-key");
    let secret = header_str(req.headers(), "x-api-secret");

    let (Some(key), Some(secret)) = (key, secret) else {
        return AppError::unauthorized("Missing X-API-Key or X-API-Secret headers").into_response();
    };

    match ApiKey::verify(&state.db, key, secret).await {
        Ok(api_key) => {
            ApiKey::record_usage(&state.db, &api_key.key).await;
            req.extensions_mut().insert(api_key);
            next.run(req).await
        }
        Err(_) => AppError::unauthorized("Invalid or revoked API key").into_response(),
    }
}

fn header_str<'a>(
    headers: &'a axum::http::HeaderMap,
    name: &str,
) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}
