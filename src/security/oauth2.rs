use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto::encryption::EncryptedString;
use crate::errors::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub token_type: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OAuth2UserInfo {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuth2Config {
    pub provider: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scopes: Vec<String>,
}

pub struct OAuth2Provider {
    config: OAuth2Config,
    client: Client,
}

impl OAuth2Provider {
    pub fn new(config: OAuth2Config) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    /// Build the authorization URL to redirect the user to.
    pub fn authorize_url(&self, state: &str) -> String {
        let scopes = self.config.scopes.join(" ");
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
            self.config.auth_url,
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(&self.config.redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state),
        )
    }

    /// Exchange an authorization code for tokens.
    pub async fn exchange_code(&self, code: &str) -> AppResult<TokenResponse> {
        let resp = self
            .client
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &self.config.redirect_uri),
                ("client_id", &self.config.client_id),
                ("client_secret", &self.config.client_secret),
            ])
            .send()
            .await
            .map_err(|_| AppError::internal())?;

        if !resp.status().is_success() {
            return Err(AppError::unauthorized("OAuth2 token exchange failed"));
        }

        resp.json::<TokenResponse>()
            .await
            .map_err(|_| AppError::internal())
    }

    /// Fetch user info from the provider using an access token.
    pub async fn get_user_info(&self, access_token: &str) -> AppResult<OAuth2UserInfo> {
        let resp = self
            .client
            .get(&self.config.userinfo_url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| AppError::internal())?;

        if !resp.status().is_success() {
            return Err(AppError::unauthorized("Failed to fetch OAuth2 user info"));
        }

        resp.json::<OAuth2UserInfo>()
            .await
            .map_err(|_| AppError::internal())
    }
}

/// Persist an OAuth2 account link in the database.
pub async fn upsert_oauth2_account(
    pool: &PgPool,
    user_id: Uuid,
    provider: &str,
    provider_user_id: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<()> {
    let access_token_enc = EncryptedString::new(access_token.to_string());
    let refresh_token_enc = refresh_token.map(|rt| EncryptedString::new(rt.to_string()));

    sqlx::query(
        "INSERT INTO oauth2_accounts (user_id, provider, provider_user_id, access_token, refresh_token, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (provider, provider_user_id)
         DO UPDATE SET access_token = $4, refresh_token = $5, expires_at = $6",
    )
    .bind(user_id)
    .bind(provider)
    .bind(provider_user_id)
    .bind(access_token_enc)
    .bind(refresh_token_enc)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}
