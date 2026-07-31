use axum::{
    extract::{Extension, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware;
use crate::models::auth::{
    AuthResponse, ChangePasswordRequest, Claims, DisableTwoFactorRequest, LoginRequest,
    LogoutRequest, RecoverTwoFactorRequest, RefreshRequest, RegisterRequest, SessionListResponse,
    SessionSummary, TwoFactorSetupResponse, VerifyTwoFactorRequest, VerifyTwoFactorResponse,
};
use crate::models::creator::Creator;
use crate::security::token_revocation;
use crate::services::auth_service;
use crate::services::token_family_service::{self, RotationOutcome};
use crate::validation::ValidatedJson;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    let protected = Router::new()
        .route("/auth/totp/enroll", post(totp_enroll))
        .route("/auth/totp/verify", post(totp_verify))
        .route("/auth/totp/disable", post(totp_disable))
        .route("/auth/backup-codes", post(regenerate_backup_codes))
        .route("/auth/logout", post(logout))
        .route("/auth/password/change", post(change_password))
        .route(
            "/auth/sessions",
            get(list_sessions).delete(revoke_all_sessions),
        )
        .route(
            "/auth/sessions/:family_id",
            axum::routing::delete(revoke_session),
        )
        .layer(axum::middleware::from_fn_with_state(
            state,
            middleware::auth::require_auth,
        ));

    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/2fa/recover", post(recover))
        .merge(protected)
}

/// Extracts the caller's IP from `X-Forwarded-For` (first hop) for device
/// metadata on refresh-token families. Best-effort only — not used for any
/// security decision.
fn client_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
}

/// Issues a brand-new refresh-token family (login/register/2FA recovery) and
/// persists its device metadata.
async fn issue_new_session(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    username: &str,
    role: &str,
) -> AppResult<AuthResponse> {
    let tv = match &state.redis {
        Some(redis) => token_revocation::current_epoch(redis, username).await,
        None => 0,
    };

    let family_id = Uuid::new_v4();
    let issued = auth_service::issue_tokens(username, role, family_id, tv)?;

    token_family_service::create_family(
        &state.db,
        username,
        issued.refresh_jti,
        user_agent(headers).as_deref(),
        client_ip(headers).as_deref(),
    )
    .await?;

    Ok(AuthResponse {
        access_token: issued.access_token,
        refresh_token: issued.refresh_token,
        token_type: "Bearer".to_owned(),
    })
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
    headers: HeaderMap,
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

    let tokens = issue_new_session(&state, &headers, &creator.username, "creator").await?;

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
    headers: HeaderMap,
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
                if let Some(idx) =
                    auth_service::verify_backup_code(backup_code, &creator.backup_code_hashes)?
                {
                    let mut remaining_codes = creator.backup_code_hashes.clone();
                    remaining_codes.remove(idx);
                    sqlx::query("UPDATE creators SET backup_code_hashes = $1 WHERE username = $2")
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

    let tokens = issue_new_session(&state, &headers, &creator.username, "creator").await?;

    Ok((StatusCode::OK, Json(serde_json::json!(tokens))).into_response())
}

/// Refresh access token using a valid refresh token. Rotates the refresh
/// token forward; presenting an already-rotated token is treated as theft
/// and revokes the entire token family (#345).
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
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<RefreshRequest>,
) -> Result<impl IntoResponse, AppError> {
    let invalid = || AppError::unauthorized("Invalid or expired refresh token");

    let claims =
        auth_service::validate_token(&body.refresh_token, "refresh").map_err(|_| invalid())?;

    let family_id = claims
        .family
        .as_deref()
        .and_then(|f| Uuid::parse_str(f).ok())
        .ok_or_else(invalid)?;
    let presented_jti = Uuid::parse_str(&claims.jti).map_err(|_| invalid())?;

    let tv = match &state.redis {
        Some(redis) => token_revocation::current_epoch(redis, &claims.sub).await,
        None => 0,
    };

    // Mint the replacement pair up front so the new jti can be handed to the
    // atomic rotate-or-detect-reuse check below. If that check doesn't come
    // back `Rotated`, these freshly minted tokens are simply discarded.
    let issued = auth_service::issue_tokens(&claims.sub, &claims.role, family_id, tv)?;

    let outcome = token_family_service::rotate_or_detect_reuse(
        &state.db,
        family_id,
        presented_jti,
        issued.refresh_jti,
    )
    .await?;

    match outcome {
        RotationOutcome::Rotated => {
            let response = AuthResponse {
                access_token: issued.access_token,
                refresh_token: issued.refresh_token,
                token_type: "Bearer".to_owned(),
            };
            Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
        }
        RotationOutcome::ReuseDetected => {
            tracing::error!(
                username = %claims.sub,
                family_id = %family_id,
                "Refresh token reuse detected — family revoked"
            );
            Err(AppError::unauthorized(
                "Refresh token reuse detected; this session family has been revoked",
            ))
        }
        RotationOutcome::AlreadyRevoked | RotationOutcome::Expired | RotationOutcome::NotFound => {
            Err(invalid())
        }
    }
}

/// Logs out the current session: denylists the presented access token and,
/// if a refresh token is supplied, revokes its token family too.
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    request_body = LogoutRequest,
    responses(
        (status = 204, description = "Logged out"),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn logout(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    body: Option<Json<LogoutRequest>>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(ref redis) = state.redis {
        let remaining = claims.exp as i64 - chrono::Utc::now().timestamp();
        token_revocation::revoke_jti(redis, &claims.jti, remaining).await;
    }

    if let Some(refresh_token) = body.and_then(|b| b.0.refresh_token) {
        if let Ok(refresh_claims) = auth_service::validate_token(&refresh_token, "refresh") {
            if refresh_claims.sub == claims.sub {
                if let Some(family_id) = refresh_claims
                    .family
                    .as_deref()
                    .and_then(|f| Uuid::parse_str(f).ok())
                {
                    let _ =
                        token_family_service::revoke_family(&state.db, family_id, "logout").await;
                }
            }
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Changes the authenticated creator's password. Invalidates every
/// outstanding access token (via the token-version epoch) and revokes all
/// refresh-token families, forcing re-authentication everywhere.
#[utoipa::path(
    post,
    path = "/auth/password/change",
    tag = "auth",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Password changed; all sessions revoked"),
        (status = 401, description = "Invalid current password")
    )
)]
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ValidatedJson(body): ValidatedJson<ChangePasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    let creator = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE username = $1",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::from)?;

    let valid = auth_service::verify_password(&body.old_password, &creator.password_hash)?;
    if !valid {
        return Err(AppError::unauthorized("Invalid current password"));
    }

    let new_hash = auth_service::hash_password(&body.new_password)?;
    sqlx::query("UPDATE creators SET password_hash = $1 WHERE username = $2")
        .bind(&new_hash)
        .bind(&claims.sub)
        .execute(&state.db)
        .await
        .map_err(AppError::from)?;

    if let Some(ref redis) = state.redis {
        token_revocation::bump_epoch(redis, &claims.sub).await;
    }
    token_family_service::revoke_all_for_user(&state.db, &claims.sub, "password_change").await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Lists the authenticated user's active sessions (refresh-token families).
#[utoipa::path(
    get,
    path = "/auth/sessions",
    tag = "auth",
    responses((status = 200, description = "Active sessions", body = SessionListResponse))
)]
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let families = token_family_service::list_active_for_user(&state.db, &claims.sub).await?;
    let sessions: Vec<SessionSummary> = families.into_iter().map(Into::into).collect();
    Ok((
        StatusCode::OK,
        Json(serde_json::json!(SessionListResponse { sessions })),
    )
        .into_response())
}

/// Revokes a single session (refresh-token family) belonging to the caller.
#[utoipa::path(
    delete,
    path = "/auth/sessions/{family_id}",
    tag = "auth",
    responses(
        (status = 204, description = "Session revoked"),
        (status = 403, description = "Not the owner of this session"),
        (status = 404, description = "Session not found")
    )
)]
pub async fn revoke_session(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(family_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let family = token_family_service::get_family(&state.db, family_id)
        .await?
        .ok_or_else(|| AppError::not_found("Session not found"))?;

    if family.username != claims.sub {
        return Err(AppError::forbidden("Cannot revoke another user's session"));
    }

    token_family_service::revoke_family(&state.db, family_id, "user_revoked").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Revokes every active session for the caller ("log out everywhere").
#[utoipa::path(
    delete,
    path = "/auth/sessions",
    tag = "auth",
    responses((status = 204, description = "All sessions revoked"))
)]
pub async fn revoke_all_sessions(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    token_family_service::revoke_all_for_user(&state.db, &claims.sub, "user_revoked_all").await?;
    if let Some(ref redis) = state.redis {
        token_revocation::bump_epoch(redis, &claims.sub).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/auth/totp/enroll",
    tag = "auth",
    responses(
        (status = 200, description = "2FA setup initiated", body = TwoFactorSetupResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "2FA already enabled")
    )
)]
pub async fn totp_enroll(
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

    use crate::crypto::encryption::EncryptedString;

    let _ = sqlx::query_as::<_, Creator>(
        "UPDATE creators SET totp_secret = $1 WHERE username = $2 RETURNING id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at",
    )
    .bind(EncryptedString::new(secret.clone()))
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
    path = "/auth/totp/verify",
    tag = "auth",
    request_body = VerifyTwoFactorRequest,
    responses(
        (status = 200, description = "2FA verified", body = VerifyTwoFactorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "2FA already enabled")
    )
)]
pub async fn totp_verify(
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

    if creator.totp_enabled {
        return Err(AppError::Conflict {
            code: "TWO_FACTOR_ALREADY_ENABLED",
            message: "Two-factor authentication is already enabled".to_string(),
        });
    }

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

    use crate::crypto::encryption::EncryptedString;

    let encrypted_hashes: Vec<EncryptedString> = backup_hashes
        .into_iter()
        .map(EncryptedString::new)
        .collect();

    sqlx::query(
        "UPDATE creators SET totp_enabled = TRUE, backup_code_hashes = $1 WHERE username = $2",
    )
    .bind(&encrypted_hashes)
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
    path = "/auth/totp/disable",
    tag = "auth",
    request_body = DisableTwoFactorRequest,
    responses(
        (status = 200, description = "TOTP disabled successfully"),
        (status = 401, description = "Unauthorized or Invalid password"),
        (status = 409, description = "TOTP not enabled")
    )
)]
pub async fn totp_disable(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ValidatedJson(body): ValidatedJson<DisableTwoFactorRequest>,
) -> Result<impl IntoResponse, AppError> {
    let creator = sqlx::query_as::<_, Creator>(
        "SELECT id, username, wallet_address, email, password_hash, totp_secret, totp_enabled, backup_code_hashes, created_at FROM creators WHERE username = $1",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "2FA disable lookup failed");
        AppError::from(e)
    })?;

    if !creator.totp_enabled {
        return Err(AppError::Conflict {
            code: "TWO_FACTOR_NOT_ENABLED",
            message: "Two-factor authentication is not enabled".to_string(),
        });
    }

    let valid =
        auth_service::verify_password(&body.password, &creator.password_hash).map_err(|e| {
            tracing::error!(error = %e, "Password verification error");
            AppError::internal()
        })?;

    if !valid {
        return Err(AppError::unauthorized("Invalid password"));
    }

    use crate::crypto::encryption::EncryptedString;
    let empty_hashes: Vec<EncryptedString> = vec![];
    let empty_secret: Option<EncryptedString> = None;

    sqlx::query(
        "UPDATE creators SET totp_enabled = FALSE, totp_secret = $1, backup_code_hashes = $2 WHERE username = $3",
    )
    .bind(&empty_secret)
    .bind(&empty_hashes)
    .bind(&claims.sub)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "2FA disable persist failed");
        AppError::from(e)
    })?;

    Ok(StatusCode::OK.into_response())
}

#[utoipa::path(
    post,
    path = "/auth/backup-codes",
    tag = "auth",
    responses(
        (status = 200, description = "Backup codes regenerated", body = VerifyTwoFactorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "TOTP not enabled")
    )
)]
pub async fn regenerate_backup_codes(
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
        tracing::error!(error = %e, "Backup codes lookup failed");
        AppError::from(e)
    })?;

    if !creator.totp_enabled {
        return Err(AppError::Conflict {
            code: "TWO_FACTOR_NOT_ENABLED",
            message: "Two-factor authentication is not enabled".to_string(),
        });
    }

    let backup_codes = auth_service::generate_backup_codes();
    let backup_hashes = backup_codes
        .iter()
        .map(|code| auth_service::hash_backup_code(code))
        .collect::<AppResult<Vec<String>>>()?;

    use crate::crypto::encryption::EncryptedString;
    let encrypted_hashes: Vec<EncryptedString> = backup_hashes
        .into_iter()
        .map(EncryptedString::new)
        .collect();

    sqlx::query("UPDATE creators SET backup_code_hashes = $1 WHERE username = $2")
        .bind(&encrypted_hashes)
        .bind(&claims.sub)
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Backup codes persist failed");
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
    headers: HeaderMap,
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

    let valid =
        auth_service::verify_password(&body.password, &creator.password_hash).map_err(|e| {
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

    let backup_index =
        auth_service::verify_backup_code(&body.backup_code, &creator.backup_code_hashes)?;
    let idx = backup_index.ok_or_else(|| AppError::unauthorized("Invalid backup code"))?;

    let mut remaining_codes = creator.backup_code_hashes.clone();
    remaining_codes.remove(idx);
    sqlx::query("UPDATE creators SET backup_code_hashes = $1 WHERE username = $2")
        .bind(&remaining_codes)
        .bind(&creator.username)
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Backup code consume failed");
            AppError::from(e)
        })?;

    let tokens = issue_new_session(&state, &headers, &creator.username, "creator").await?;

    Ok((StatusCode::OK, Json(serde_json::json!(tokens))).into_response())
}
