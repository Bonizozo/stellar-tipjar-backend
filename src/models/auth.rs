use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

/// Register a new creator account with a password.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RegisterRequest {
    #[validate(length(
        min = 3,
        max = 30,
        message = "Username must be between 3 and 30 characters"
    ))]
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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totp_code: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_code: Option<String>,
}

/// Setup a new TOTP secret for the authenticated creator.
#[derive(Debug, Serialize, ToSchema)]
pub struct TwoFactorSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
}

/// Verify a TOTP code to finish enrollment and receive backup codes.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct VerifyTwoFactorRequest {
    #[validate(length(min = 6, max = 6, message = "TOTP code must be 6 digits"))]
    pub totp_code: String,
}

/// Backup code payload returned after verifying 2FA enrollment.
#[derive(Debug, Serialize, ToSchema)]
pub struct VerifyTwoFactorResponse {
    pub backup_codes: Vec<String>,
}

/// Recover access with username, password, and a single backup code.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RecoverTwoFactorRequest {
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[validate(length(min = 1, message = "Password is required"))]
    pub password: String,

    #[validate(length(min = 1, message = "Backup code is required"))]
    pub backup_code: String,
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
    /// Role string: "creator" | "supporter" | "admin" | "moderator" | "guest"
    pub role: String,
    pub exp: usize,
    pub iat: usize,
    pub nbf: usize,
    /// Issuer — validated against the configured issuer at decode time.
    pub iss: String,
    /// Audience — validated against the configured audience at decode time.
    pub aud: String,
    /// Unique token id, used for access-token revocation and refresh reuse detection.
    pub jti: String,
    /// Refresh-token family id. Present on refresh tokens only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Token version — must be >= the user's current epoch in the revocation
    /// store or the token is treated as revoked (used by logout-all /
    /// password-change / admin revocation).
    #[serde(default)]
    pub tv: i64,
}

/// Disable TOTP for the authenticated creator.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct DisableTwoFactorRequest {
    #[validate(length(min = 1, message = "Password is required"))]
    pub password: String,
}

/// Logout the current session; optionally revoke the refresh-token family too.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct LogoutRequest {
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// Change the authenticated creator's password. Invalidates all outstanding
/// access tokens and refresh-token families for the account.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ChangePasswordRequest {
    #[validate(length(min = 1, message = "Current password is required"))]
    pub old_password: String,

    #[validate(length(min = 8, message = "New password must be at least 8 characters"))]
    pub new_password: String,
}

/// A single active session (refresh-token family) belonging to a user.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionSummary {
    pub family_id: uuid::Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub rotated_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
}
