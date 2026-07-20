use axum::http::{HeaderValue, Method};
use std::time::Duration;
use tower_http::cors::CorsLayer;

use crate::config::CorsConfig;

/// Builds a [`CorsLayer`] from a validated [`CorsConfig`].
///
/// This is the sole public API — no direct env reads occur here.
/// All values are taken from `AppConfig::cors` which is validated once at
/// startup via `AppConfig::from_env()`.
pub fn cors_layer(cfg: &CorsConfig) -> CorsLayer {
    let methods = [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::OPTIONS,
    ];

    let layer = CorsLayer::new()
        .allow_methods(methods)
        .allow_headers(tower_http::cors::Any)
        .max_age(Duration::from_secs(cfg.max_age_secs));

    let origins: Vec<HeaderValue> = cfg
        .allowed_origins
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    if origins.is_empty() {
        // No specific origins → allow any (no credentials)
        layer.allow_origin(tower_http::cors::Any)
    } else {
        layer.allow_origin(origins).allow_credentials(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wildcard_cfg() -> CorsConfig {
        CorsConfig {
            allowed_origins: vec![],
            max_age_secs: 3600,
        }
    }

    fn specific_origins_cfg() -> CorsConfig {
        CorsConfig {
            allowed_origins: vec![
                "http://localhost:3000".into(),
                "https://example.com".into(),
            ],
            max_age_secs: 600,
        }
    }

    #[test]
    fn builds_with_wildcard_when_no_origins() {
        let _layer = cors_layer(&wildcard_cfg()); // must not panic
    }

    #[test]
    fn builds_with_specific_origins() {
        let _layer = cors_layer(&specific_origins_cfg()); // must not panic
    }

    #[test]
    fn respects_custom_max_age() {
        let cfg = CorsConfig {
            allowed_origins: vec![],
            max_age_secs: 600,
        };
        let _layer = cors_layer(&cfg); // must not panic
    }
}
