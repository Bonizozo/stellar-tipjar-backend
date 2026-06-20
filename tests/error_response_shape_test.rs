//! Verifies that every endpoint error follows the standard envelope:
//! `{ error: string, code: string, status: number, details?: object, request_id?: string }`.
//!
//! This is a self-contained middleware test: it builds a minimal router (no
//! database) that wraps handlers returning each `AppError` variant with the
//! same request-id middleware used in production, so it exercises the real
//! `propagate_request_id` + `AppError::into_response` pipeline.

use axum::{
    body::{to_bytes, Body},
    http::{HeaderName, Request, StatusCode},
    routing::get,
    Router,
};
use stellar_tipjar_backend::errors::{AppError, StellarError};
use tower::ServiceExt;

async fn bad_request() -> Result<&'static str, AppError> {
    Err(AppError::bad_request("missing field 'amount'"))
}

async fn unauthorized() -> Result<&'static str, AppError> {
    Err(AppError::unauthorized("missing or invalid token"))
}

async fn not_found() -> Result<&'static str, AppError> {
    Err(AppError::not_found("creator"))
}

async fn unprocessable() -> Result<&'static str, AppError> {
    Err(AppError::Stellar(StellarError::InvalidTransaction {
        reason: "signature mismatch".to_string(),
    }))
}

async fn too_many_requests() -> Result<&'static str, AppError> {
    Err(AppError::rate_limited_with_retry("slow down", 30))
}

async fn internal_error() -> Result<&'static str, AppError> {
    Err(AppError::internal())
}

fn test_app() -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");

    Router::new()
        .route("/bad-request", get(bad_request))
        .route("/unauthorized", get(unauthorized))
        .route("/not-found", get(not_found))
        .route("/unprocessable", get(unprocessable))
        .route("/too-many-requests", get(too_many_requests))
        .route("/internal-error", get(internal_error))
        .layer(axum::middleware::from_fn(
            stellar_tipjar_backend::middleware::request_id::propagate_request_id,
        ))
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            x_request_id.clone(),
            tower_http::request_id::MakeRequestUuid,
        ))
        .layer(tower_http::request_id::PropagateRequestIdLayer::new(
            x_request_id,
        ))
}

/// Sends a request through a fresh router instance and returns (status, json body, x-request-id header).
async fn fetch(path: &str) -> (StatusCode, serde_json::Value, Option<String>) {
    let response = test_app()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();

    let status = response.status();
    let header_request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    (status, json, header_request_id)
}

/// Asserts the common envelope shape every error response must satisfy.
fn assert_envelope_shape(json: &serde_json::Value, expected_status: StatusCode, expected_code: &str) {
    assert!(json["error"].is_string(), "`error` must be a string: {json}");
    assert_eq!(json["code"], expected_code);
    assert_eq!(json["status"], expected_status.as_u16());
    assert!(
        json["request_id"].is_string(),
        "`request_id` must be present in the body: {json}"
    );
}

#[tokio::test]
async fn returns_structured_400_for_bad_request() {
    let (status, json, header_id) = fetch("/bad-request").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_envelope_shape(&json, StatusCode::BAD_REQUEST, "INVALID_REQUEST");
    assert_eq!(json["request_id"], header_id.unwrap());
}

#[tokio::test]
async fn returns_structured_401_for_unauthorized() {
    let (status, json, header_id) = fetch("/unauthorized").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_envelope_shape(&json, StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    assert_eq!(json["request_id"], header_id.unwrap());
}

#[tokio::test]
async fn returns_structured_404_for_not_found() {
    let (status, json, header_id) = fetch("/not-found").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_envelope_shape(&json, StatusCode::NOT_FOUND, "DB_NOT_FOUND");
    assert_eq!(json["details"]["entity"], "creator");
    assert_eq!(json["request_id"], header_id.unwrap());
}

#[tokio::test]
async fn returns_structured_422_for_unprocessable_entity() {
    let (status, json, header_id) = fetch("/unprocessable").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_envelope_shape(&json, StatusCode::UNPROCESSABLE_ENTITY, "STELLAR_INVALID_TX");
    assert_eq!(json["details"]["reason"], "signature mismatch");
    assert_eq!(json["request_id"], header_id.unwrap());
}

#[tokio::test]
async fn returns_structured_429_for_rate_limited() {
    let (status, json, header_id) = fetch("/too-many-requests").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_envelope_shape(&json, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT_EXCEEDED");
    assert_eq!(json["details"]["retry_after_secs"], 30);
    assert_eq!(json["request_id"], header_id.unwrap());
}

#[tokio::test]
async fn returns_structured_500_for_internal_error() {
    let (status, json, header_id) = fetch("/internal-error").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_envelope_shape(&json, StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR");
    // Internal errors must never leak implementation details.
    assert!(json.get("details").is_none() || json["details"].is_null());
    assert_eq!(json["request_id"], header_id.unwrap());
}
