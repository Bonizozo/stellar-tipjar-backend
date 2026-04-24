use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};

const SUNSET_DATE: &str = "Sat, 01 Jan 2027 00:00:00 GMT";
const MIGRATION_LINK: &str =
    r#"<https://docs.example.com/migration/v1-to-v2>; rel="deprecation""#;

/// Injects `X-API-Version` on every response.
/// For v1 paths, also adds deprecation / sunset headers.
pub async fn version_headers(req: Request, next: Next) -> Response {
    let version = detect_version(req.uri().path());
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        "X-API-Version",
        HeaderValue::from_static(version),
    );

    if version == "v1" {
        headers.insert("Deprecation", HeaderValue::from_static("true"));
        headers.insert("Sunset", HeaderValue::from_static(SUNSET_DATE));
        headers.insert(
            "Link",
            HeaderValue::from_static(MIGRATION_LINK),
        );
    }

    response
}

fn detect_version(path: &str) -> &'static str {
    if path.contains("/v1/") || path.ends_with("/v1") {
        "v1"
    } else {
        "v2"
    }
}
