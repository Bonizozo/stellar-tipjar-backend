use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

use crate::errors::{AppError, AppResult};
use crate::models::auth::{AuthResponse, Claims};

const ACCESS_TOKEN_SECS: i64 = 60 * 60 * 24;
const REFRESH_TOKEN_SECS: i64 = 60 * 60 * 24 * 7;

fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").expect("JWT_SECRET must be set")
}

#[tracing::instrument(skip_all, fields(username = %username))]
pub fn generate_tokens(username: &str) -> AppResult<AuthResponse> {
    let secret = jwt_secret();
    let now = Utc::now().timestamp() as usize;

    let access_claims = Claims {
        sub: username.to_owned(),
        kind: "access".to_owned(),
        exp: now + ACCESS_TOKEN_SECS as usize,
        iat: now,
    };

    let refresh_claims = Claims {
        sub: username.to_owned(),
        kind: "refresh".to_owned(),
        exp: now + REFRESH_TOKEN_SECS as usize,
        iat: now,
    };

    let access_token = encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!(error = %e, "Refresh token generation failed");
        AppError::internal()
    })?;

    tracing::debug!("Tokens generated");
    Ok(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_owned(),
    })
}

#[tracing::instrument(skip_all)]
pub fn validate_token(token: &str, expected_kind: &str) -> AppResult<Claims> {
    let secret = jwt_secret();
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        tracing::warn!(error = %e, "Token validation failed");
        AppError::Unauthorized {
            message: "Invalid or expired token".to_string(),
        }
    })?;

    if token_data.claims.kind != expected_kind {
        tracing::warn!(expected = %expected_kind, got = %token_data.claims.kind, "Wrong token kind");
        return Err(AppError::Unauthorized {
            message: "Invalid token kind".to_string(),
        });
    }

    Ok(token_data.claims)
}

#[tracing::instrument(skip_all)]
pub fn hash_password(password: &str) -> AppResult<String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| {
        tracing::error!(error = %e, "Password hashing failed");
        AppError::internal()
    })
}

#[tracing::instrument(skip_all)]
pub fn verify_password(password: &str, hash: &str) -> AppResult<bool> {
    bcrypt::verify(password, hash).map_err(|e| {
        tracing::error!(error = %e, "Password verification failed");
        AppError::internal()
    })
}
