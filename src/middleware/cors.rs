use std::time::Duration;

use axum::http::{HeaderName, HeaderValue, Method, Response};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn cors_layer() -> CorsLayer {
    let allowed_origins = std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let origins: Vec<HeaderValue> = allowed_origins
        .split(',')
        .filter_map(|s| HeaderValue::from_str(s.trim()).ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
        ])
        .allow_credentials(true)
        .max_age(Duration::from_secs(3600))
}

pub async fn security_headers<B>(response: Response<B>) -> Response<B> {
    let (mut parts, body) = response.into_parts();
    let h = &mut parts.headers;

    h.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'self'"),
    );
    h.insert(
        HeaderName::from_static("strict-transport-security"),
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    h.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    h.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );

    Response::from_parts(parts, body)
}
