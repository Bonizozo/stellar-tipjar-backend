use axum::{extract::{Extension, State}, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use std::sync::Arc;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware;
use crate::models::auth::{
    AuthResponse, LoginRequest, RefreshRequest, RegisterRequest, Claims,
    RecoverTwoFactorRequest, TwoFactorSetupResponse, VerifyTwoFactorRequest,
    VerifyTwoFactorResponse,
};
use crate::models::creator::Creator;
use crate::services::auth_service;
use crate::validation::ValidatedJson;

pub fn router() -> Router<Arc<AppState>> {
    let protected = Router::new()
        .route("/auth/2fa/setup", post(setup_2fa))
        .route("/auth/2fa/verify", post(verify_2fa))
        .layer(axum::middleware::from_fn(middleware::auth::require_auth));

    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/2fa/recover", post(recover))
        .merge(protected)
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
pub async fn register(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    let password_hash = auth_service::hash_password(&body.password).map_err(|e| {
        tracing::error!(error = %e, "Password hashing failed");
        AppError::internal()
    })?;

    let creator = sqlx::query_as::<_, Creator>(
        r#"
        INSERT INTO creators (id, username, wallet_address, password_hash, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        RETURNING id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at
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

    let tokens = auth_service::generate_tokens(&creator.username, "creator").map_err(|e| {
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
pub async fn login(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, totp_secret, totp_enabled, backup_code_hashes, created_at, password_hash FROM creators WHERE username = $1",
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

    let valid =
        auth_service::verify_password(&body.password, &creator.password_hash).map_err(|e| {
            tracing::error!(error = %e, "Password verification error");
            AppError::internal()
        })?;
    if !valid {
        return Err(AppError::unauthorized("Invalid credentials"));
    }

    if creator.totp_enabled {
        let mut two_factor_valid = false;

        if let Some(ref totp_code) = body.totp_code {
            if let Some(ref secret) = creator.totp_secret {
                two_factor_valid = auth_service::validate_totp_code(secret, totp_code)?;
            }
        }

        if !two_factor_valid {
            if let Some(ref backup_code) = body.backup_code {
                if let Some(idx) = auth_service::verify_backup_code(backup_code, &creator.backup_code_hashes)? {
                    let mut remaining_codes = creator.backup_code_hashes.clone();
                    remaining_codes.remove(idx);
                    sqlx::query(
                        "UPDATE creators SET backup_code_hashes = $1 WHERE username = $2",
                    )
                    .bind(&remaining_codes)
                    .bind(&creator.username)
                    .execute(&state.db)
                    .await
                    .map_err(|e| {
                        tracing::error!(error = %e, "Backup code consume failed");
                        AppError::from(e)
                    })?;
                    two_factor_valid = true;
                }
            }
        }

        if !two_factor_valid {
            return Err(AppError::unauthorized(
                "Two-factor code or backup code is required",
            ));
        }
    }

    let tokens = auth_service::generate_tokens(&creator.username, "creator").map_err(|e| {
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
pub async fn refresh(
    ValidatedJson(body): ValidatedJson<RefreshRequest>,
) -> Result<impl IntoResponse, AppError> {
    let claims = auth_service::validate_token(&body.refresh_token, "refresh")
        .map_err(|_| AppError::unauthorized("Invalid or expired refresh token"))?;

    let tokens = auth_service::generate_tokens(&claims.sub, &claims.role).map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    Ok((StatusCode::OK, Json(serde_json::json!(tokens))).into_response())
}

#[utoipa::path(
    post,
    path = "/auth/2fa/setup",
    tag = "auth",
    responses(
        (status = 200, description = "2FA setup initiated", body = TwoFactorSetupResponse),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn setup_2fa(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let creator = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE username = $1",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "2FA setup lookup failed");
        AppError::from(e)
    })?;

    if creator.totp_enabled {
        return Err(AppError::Conflict {
            code: "TWO_FACTOR_ALREADY_ENABLED",
            message: "Two-factor authentication is already enabled".to_string(),
        });
    }

    let secret = auth_service::generate_totp_secret().map_err(|e| {
        tracing::error!(error = %e, "2FA secret generation failed");
        AppError::internal()
    })?;

    let creator = sqlx::query_as::<_, Creator>(
        "UPDATE creators SET totp_secret = $1 WHERE username = $2 RETURNING id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at",
    )
    .bind(&secret)
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "2FA setup failed");
        AppError::from(e)
    })?;

    let otpauth_url = format!(
        "otpauth://totp/StellarTipJar:{}?secret={}&issuer=StellarTipJar&digits=6&period=30",
        creator.username, secret
    );

    Ok((
        StatusCode::OK,
        Json(serde_json::json!(TwoFactorSetupResponse {
            secret,
            otpauth_url,
        })),
    )
        .into_response())
}

#[utoipa::path(
    post,
    path = "/auth/2fa/verify",
    tag = "auth",
    request_body = VerifyTwoFactorRequest,
    responses(
        (status = 200, description = "2FA verified", body = VerifyTwoFactorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "2FA already enabled")
    )
)]
pub async fn verify_2fa(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ValidatedJson(body): ValidatedJson<VerifyTwoFactorRequest>,
) -> Result<impl IntoResponse, AppError> {
    let creator = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE username = $1",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "2FA verification lookup failed");
        AppError::from(e)
    })?;

    let secret = creator
        .totp_secret
        .as_ref()
        .ok_or_else(|| AppError::unauthorized("Two-factor setup has not been initiated"))?;

    if !auth_service::validate_totp_code(secret, &body.totp_code)? {
        return Err(AppError::unauthorized("Invalid two-factor code"));
    }

    let backup_codes = auth_service::generate_backup_codes();
    let backup_hashes = backup_codes
        .iter()
        .map(|code| auth_service::hash_backup_code(code))
        .collect::<AppResult<Vec<String>>>()?;

    sqlx::query(
        "UPDATE creators SET totp_enabled = TRUE, backup_code_hashes = $1 WHERE username = $2",
    )
    .bind(&backup_hashes)
    .bind(&claims.sub)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "2FA verification persist failed");
        AppError::from(e)
    })?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!(VerifyTwoFactorResponse { backup_codes })),
    )
        .into_response())
}

#[utoipa::path(
    post,
    path = "/auth/2fa/recover",
    tag = "auth",
    request_body = RecoverTwoFactorRequest,
    responses(
        (status = 200, description = "Account recovered", body = AuthResponse),
        (status = 401, description = "Invalid credentials or backup code")
    )
)]
pub async fn recover(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<RecoverTwoFactorRequest>,
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE username = $1",
    )
    .bind(&body.username)
    .fetch_optional(&state.db)
    .await;

    let creator = match row {
        Ok(Some(c)) => c,
        Ok(None) => return Err(AppError::unauthorized("Invalid credentials")),
        Err(e) => {
            tracing::error!(error = %e, "Recovery DB error");
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

    if !creator.totp_enabled {
        return Err(AppError::unauthorized(
            "Two-factor authentication is not enabled for this account",
        ));
    }

    let backup_index = auth_service::verify_backup_code(&body.backup_code, &creator.backup_code_hashes)?;
    let idx = backup_index.ok_or_else(|| AppError::unauthorized("Invalid backup code"))?;

    let mut remaining_codes = creator.backup_code_hashes.clone();
    remaining_codes.remove(idx);
    sqlx::query(
        "UPDATE creators SET backup_code_hashes = $1 WHERE username = $2",
    )
    .bind(&remaining_codes)
    .bind(&creator.username)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Backup code consume failed");
        AppError::from(e)
    })?;

    let tokens = auth_service::generate_tokens(&creator.username, "creator").map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    Ok((StatusCode::OK, Json(serde_json::json!(tokens))).into_response())
}
