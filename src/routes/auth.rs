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
use crate::errors::AppError;
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
) -> Result<impl IntoResponse, AppError> {
    let password_hash = auth_service::hash_password(&body.password)
        .map_err(|e| {
            tracing::error!(error = %e, "Password hashing failed");
            AppError::internal()
        })?;

    let creator = sqlx::query_as::<_, Creator>(
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
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.code().as_deref() == Some("23505") {
                return AppError::Conflict {
                    code: "USERNAME_TAKEN",
                    message: "Username already taken".to_string(),
                };
            }
        }
        tracing::error!(error = %e, "Registration failed");
        AppError::from(e)
    })?;

    let tokens = auth_service::generate_tokens(&creator.username).map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    Ok((StatusCode::CREATED, Json(serde_json::json!(tokens))).into_response())
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
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, password_hash, created_at FROM creators WHERE username = $1",
    )
    .bind(&body.username)
    .fetch_optional(&state.db)
    .await;

    let creator = match row {
        Ok(Some(c)) => c,
        Ok(None) => return Err(AppError::unauthorized("Invalid credentials")),
        Err(e) => {
            tracing::error!(error = %e, "Login DB error");
            return Err(AppError::from(e));
        }
    };

    let valid = auth_service::verify_password(&body.password, &creator.password_hash).map_err(|e| {
        tracing::error!(error = %e, "Password verification error");
        AppError::internal()
    })?;
    if !valid {
        return Err(AppError::unauthorized("Invalid credentials"));
    }

    let tokens = auth_service::generate_tokens(&creator.username).map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    Ok((StatusCode::OK, Json(serde_json::json!(tokens))).into_response())
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
async fn refresh(ValidatedJson(body): ValidatedJson<RefreshRequest>) -> Result<impl IntoResponse, AppError> {
    let claims = auth_service::validate_token(&body.refresh_token, "refresh")
        .map_err(|_| AppError::unauthorized("Invalid or expired refresh token"))?;

    let tokens = auth_service::generate_tokens(&claims.sub).map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    Ok((StatusCode::OK, Json(serde_json::json!(tokens))).into_response())
}
