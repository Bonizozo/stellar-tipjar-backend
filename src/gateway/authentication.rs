use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct GatewayAuthConfig {
    pub api_keys: HashMap<String, ApiKeyInfo>,
    pub jwt_secret: String,
}

#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    pub key: String,
    pub scopes: Vec<String>,
    pub rate_limit: i32,
}

impl GatewayAuthConfig {
    pub fn new(jwt_secret: String) -> Self {
        Self {
            api_keys: HashMap::new(),
            jwt_secret,
        }
    }

    pub fn add_api_key(&mut self, key: String, scopes: Vec<String>, rate_limit: i32) {
        self.api_keys.insert(
            key.clone(),
            ApiKeyInfo {
                key,
                scopes,
                rate_limit,
            },
        );
    }

    pub fn validate_api_key(&self, key: &str) -> Option<&ApiKeyInfo> {
        self.api_keys.get(key)
    }
}

pub async fn gateway_auth_middleware<B>(
    req: Request<B>,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let _token = &header[7..];
            // Token validation would happen here
            Ok(next.run(req).await)
        }
        Some(header) if header.starts_with("ApiKey ") => {
            let _key = &header[7..];
            // API key validation would happen here
            Ok(next.run(req).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
