use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http_body_util::BodyExt;
use uuid::Uuid;

use super::fingerprint::compute_fingerprint;
use super::service::Outcome;
use super::store::StoredResponse;
use crate::db::connection::AppState;
use crate::errors::AppError;

const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const REPLAY_HEADER: &str = "X-Idempotent-Replay";

/// Opt-in per route: attach with
/// `.route_layer(axum::middleware::from_fn_with_state(state, idempotency_middleware))`
/// on just the routers/routes that should get Idempotency-Key semantics.
///
/// A request is only intercepted when both (a) its method mutates state
/// (POST/PUT/PATCH/DELETE) and (b) the client actually sent an
/// `Idempotency-Key` header — the header is opt-in from the caller's side,
/// mirroring Stripe's client SDKs. Everything else passes straight through,
/// which is what makes it safe to mount this on a router that also serves
/// GET routes for the same resource.
pub async fn idempotency_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    if !matches!(method, Method::POST | Method::PUT | Method::PATCH | Method::DELETE) {
        return next.run(request).await;
    }

    let Some(raw_key) = extract_idempotency_key(request.headers()) else {
        return next.run(request).await;
    };

    if Uuid::parse_str(&raw_key).is_err() {
        return AppError::bad_request("Idempotency-Key must be a valid UUID").into_response();
    }

    let principal = resolve_principal(&request);
    let route = format!("{} {}", method, request.uri().path());

    let (parts, body) = request.into_parts();
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return AppError::bad_request("Invalid request body").into_response(),
    };
    let fingerprint = compute_fingerprint(method.as_str(), parts.uri.path(), &body_bytes);
    let request = Request::from_parts(parts, Body::from(body_bytes));

    let outcome = match state
        .idempotency
        .begin(&principal, &route, &raw_key, &fingerprint)
        .await
    {
        Ok(outcome) => outcome,
        Err(e) => {
            // Both Redis and Postgres are unavailable. Fail open rather than
            // blocking money-adjacent traffic entirely — the request executes
            // without an idempotency guarantee, which is logged for alerting.
            tracing::error!(error = %e, principal, route, "Idempotency store unavailable; proceeding without a guarantee");
            return next.run(request).await;
        }
    };

    match outcome {
        Outcome::Replay(stored) => build_replay_response(stored),
        Outcome::Mismatch => AppError::IdempotencyKeyReused {
            message: "This Idempotency-Key was already used with a different request body"
                .to_string(),
        }
        .into_response(),
        Outcome::Conflict { retry_after_secs } => {
            AppError::IdempotencyKeyLocked { retry_after_secs }.into_response()
        }
        Outcome::Proceed(guard) => {
            let response = next.run(request).await;
            let (parts, body) = response.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    if let Err(e) = state.idempotency.fail(guard).await {
                        tracing::error!(error = %e, "Idempotency: failed to release lock after response-read error");
                    }
                    return AppError::internal().into_response();
                }
            };

            let content_type = parts
                .headers
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);

            let stored = StoredResponse {
                status: parts.status.as_u16(),
                content_type,
                body: body_bytes.to_vec(),
            };

            if let Err(e) = state.idempotency.complete(guard, stored).await {
                tracing::error!(error = %e, "Idempotency: failed to persist completed response");
            }

            let mut response = Response::from_parts(parts, Body::from(body_bytes));
            response
                .headers_mut()
                .insert(REPLAY_HEADER, HeaderValue::from_static("false"));
            response
        }
    }
}

fn build_replay_response(stored: StoredResponse) -> Response {
    let status = StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status).header(REPLAY_HEADER, "true");
    if let Some(ct) = &stored.content_type {
        builder = builder.header(axum::http::header::CONTENT_TYPE, ct);
    }
    builder
        .body(Body::from(stored.body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn extract_idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Scopes the idempotency key to the caller. Prefers the authenticated
/// principal (JWT `sub`, injected into request extensions by
/// [`crate::middleware::auth::require_auth`]); several of the endpoints this
/// middleware targets (public tip/refund creation) have no auth requirement,
/// so it falls back to client IP and finally an unscoped bucket so the
/// middleware degrades gracefully rather than refusing to run.
fn resolve_principal(request: &Request) -> String {
    if let Some(claims) = request.extensions().get::<crate::models::auth::Claims>() {
        return format!("user:{}", claims.sub);
    }
    if let Some(ip) = crate::routes::tips::extract_client_ip(request.headers()) {
        return format!("ip:{ip}");
    }
    "anon:unscoped".to_string()
}
