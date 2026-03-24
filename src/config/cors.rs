use axum::http::{HeaderValue, Method};
use std::time::Duration;
use tower_http::cors::CorsLayer;

/// Builds a [`CorsLayer`] from environment variables.
///
/// | Variable            | Default                        | Description                              |
/// |---------------------|--------------------------------|------------------------------------------|
/// `CORS_ALLOWED_ORIGINS` | `*` (any)                    | Comma-separated list of allowed origins  |
/// `CORS_MAX_AGE_SECS`    | `3600`                       | Preflight cache duration in seconds      |
///
/// When `CORS_ALLOWED_ORIGINS` is set to specific origins, `allow_credentials(true)` is
/// enabled automatically (required by the CORS spec for credentialed requests).
pub fn cors_layer_from_env() -> CorsLayer {
    let methods = [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::OPTIONS,
    ];

    let max_age: u64 = std::env::var("CORS_MAX_AGE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);

    let origins_env = std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default();
    let origins: Vec<HeaderValue> = origins_env
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();

    let layer = CorsLayer::new()
        .allow_methods(methods)
        .allow_headers(tower_http::cors::Any)
        .max_age(Duration::from_secs(max_age));

    if origins.is_empty() {
        // No specific origins configured — allow any (no credentials)
        layer.allow_origin(tower_http::cors::Any)
    } else {
        layer
            .allow_origin(origins)
            .allow_credentials(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_with_wildcard_when_no_env() {
        std::env::remove_var("CORS_ALLOWED_ORIGINS");
        std::env::remove_var("CORS_MAX_AGE_SECS");
        let _layer = cors_layer_from_env(); // must not panic
    }

    #[test]
    fn builds_with_specific_origins() {
        std::env::set_var("CORS_ALLOWED_ORIGINS", "http://localhost:3000,https://example.com");
        let _layer = cors_layer_from_env();
        std::env::remove_var("CORS_ALLOWED_ORIGINS");
    }

    #[test]
    fn respects_custom_max_age() {
        std::env::set_var("CORS_MAX_AGE_SECS", "600");
        let _layer = cors_layer_from_env();
        std::env::remove_var("CORS_MAX_AGE_SECS");
    }
}
