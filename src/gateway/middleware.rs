use crate::errors::app_error::AppError;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiVersion {
    pub version: String,
    pub deprecated: bool,
    pub sunset_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

#[derive(Debug, Clone)]
pub struct RouteConfig {
    pub path: String,
    pub methods: Vec<String>,
    pub rate_limit: RateLimitConfig,
    pub requires_auth: bool,
    pub api_versions: Vec<ApiVersion>,
}

pub struct ApiGatewayMiddleware {
    routes: Arc<RwLock<HashMap<String, RouteConfig>>>,
    rate_limiters: Arc<RwLock<HashMap<String, RateLimiter>>>,
}

#[derive(Debug, Clone)]
struct RateLimiter {
    requests: Vec<std::time::Instant>,
    limit: u32,
}

impl RateLimiter {
    fn new(limit: u32) -> Self {
        Self {
            requests: Vec::new(),
            limit,
        }
    }

    fn check_limit(&mut self) -> bool {
        let now = std::time::Instant::now();
        let one_minute_ago = now - std::time::Duration::from_secs(60);

        self.requests.retain(|&t| t > one_minute_ago);

        if self.requests.len() < self.limit as usize {
            self.requests.push(now);
            true
        } else {
            false
        }
    }
}

impl ApiGatewayMiddleware {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            rate_limiters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_route(&self, config: RouteConfig) {
        let mut routes = self.routes.write().await;
        routes.insert(config.path.clone(), config.clone());

        let mut limiters = self.rate_limiters.write().await;
        limiters.insert(
            config.path.clone(),
            RateLimiter::new(config.rate_limit.requests_per_minute),
        );
    }

    pub async fn check_rate_limit(&self, path: &str, client_id: &str) -> Result<(), AppError> {
        let key = format!("{}:{}", path, client_id);
        let mut limiters = self.rate_limiters.write().await;

        if let Some(limiter) = limiters.get_mut(&key) {
            if !limiter.check_limit() {
                return Err(AppError::rate_limited(
                    "Rate limit exceeded".to_string(),
                ));
            }
        }

        Ok(())
    }

    pub async fn transform_request(
        &self,
        path: &str,
        version: &str,
    ) -> Result<String, AppError> {
        let routes = self.routes.read().await;

        if let Some(route) = routes.get(path) {
            let version_exists = route
                .api_versions
                .iter()
                .any(|v| v.version == version && !v.deprecated);

            if version_exists {
                Ok(format!("{}/{}", version, path))
            } else {
                Err(AppError::bad_request(format!(
                    "API version {} not supported",
                    version
                )))
            }
        } else {
            Err(AppError::not_found(format!("Route {} not found", path)))
        }
    }

    pub async fn get_route_config(&self, path: &str) -> Result<RouteConfig, AppError> {
        let routes = self.routes.read().await;
        routes
            .get(path)
            .cloned()
            .ok_or_else(|| AppError::not_found(format!("Route {} not found", path)))
    }

    pub async fn list_routes(&self) -> Vec<RouteConfig> {
        let routes = self.routes.read().await;
        routes.values().cloned().collect()
    }
}

impl Default for ApiGatewayMiddleware {
    fn default() -> Self {
        Self::new()
    }
}
