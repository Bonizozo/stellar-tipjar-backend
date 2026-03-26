use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::models::auth::{AuthResponse, LoginRequest, RefreshRequest, RegisterRequest};
use crate::models::creator::Creator;
use crate::services::auth_service;
use crate::validation::ValidatedJson;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
}

/// Register a new creator account
#[utoipa::path(
    post,
    path = "/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "Registered successfully", body = AuthResponse),
        (status = 409, description = "Username already taken"),
        (status = 500, description = "Internal server error")
    )
)]
async fn register(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<RegisterRequest>,
) -> impl IntoResponse {
    let password_hash = match auth_service::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Password hashing failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response();
        }
    };

    let result = sqlx::query_as::<_, Creator>(
        r#"
        INSERT INTO creators (id, username, wallet_address, password_hash, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        RETURNING id, username, wallet_address, password_hash, created_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&body.username)
    .bind(&body.wallet_address)
    .bind(&password_hash)
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(creator) => match auth_service::generate_tokens(&creator.username) {
            Ok(tokens) => (StatusCode::CREATED, Json(serde_json::json!(tokens))).into_response(),
            Err(e) => {
                tracing::error!("Token generation failed: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response()
            }
        },
        Err(e) if e.to_string().contains("unique") || e.to_string().contains("duplicate") => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Username already taken" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Registration failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

/// Login with username and password
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = AuthResponse),
        (status = 401, description = "Invalid credentials"),
        (status = 500, description = "Internal server error")
    )
)]
async fn login(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> impl IntoResponse {
    let row = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, password_hash, created_at FROM creators WHERE username = $1",
    )
    .bind(&body.username)
    .fetch_optional(&state.db)
    .await;

    let creator = match row {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Invalid credentials" })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Login DB error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response();
        }
    };

    match auth_service::verify_password(&body.password, &creator.password_hash) {
        Ok(true) => match auth_service::generate_tokens(&creator.username) {
            Ok(tokens) => (StatusCode::OK, Json(serde_json::json!(tokens))).into_response(),
            Err(e) => {
                tracing::error!("Token generation failed: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response()
            }
        },
        Ok(false) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid credentials" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Password verification error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response()
        }
    }
}

/// Refresh access token using a valid refresh token
#[utoipa::path(
    post,
    path = "/auth/refresh",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Token refreshed", body = AuthResponse),
        (status = 401, description = "Invalid or expired refresh token")
    )
)]
async fn refresh(ValidatedJson(body): ValidatedJson<RefreshRequest>) -> impl IntoResponse {
    match auth_service::validate_token(&body.refresh_token, "refresh") {
        Ok(claims) => match auth_service::generate_tokens(&claims.sub) {
            Ok(tokens) => (StatusCode::OK, Json(serde_json::json!(tokens))).into_response(),
            Err(e) => {
                tracing::error!("Token generation failed: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Internal server error" })),
                )
                    .into_response()
            }
        },
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid or expired refresh token" })),
        )
            .into_response(),
    }
}
