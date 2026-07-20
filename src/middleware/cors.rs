use axum::http::{HeaderValue, Method};
use std::time::Duration;
use tower_http::cors::CorsLayer;

use crate::config::CorsConfig;

/// Builds a [`CorsLayer`] from a validated [`CorsConfig`].
///
/// This replaces the old `cors_layer_from_env()` that read env vars per-call.
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
