use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

/// Register a new creator account with a password.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RegisterRequest {
    #[validate(length(min = 3, max = 30, message = "Username must be between 3 and 30 characters"))]
    pub username: String,

    #[validate(custom(function = "crate::validation::stellar::validate_stellar_address"))]
    pub wallet_address: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,
}

/// Login with username + password.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct LoginRequest {
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[validate(length(min = 1, message = "Password is required"))]
    pub password: String,
}

/// Returned on successful login or register.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
}

/// Refresh access token using a refresh token.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RefreshRequest {
    #[validate(length(min = 1, message = "Refresh token is required"))]
    pub refresh_token: String,
}

/// JWT claims payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub kind: String,
    pub exp: usize,
    pub iat: usize,
}
