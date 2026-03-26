use axum::{
    extract::Request,
    http::HeaderValue,
    middleware::Next,
    response::Response,
};

/// Injects `Deprecation` and `Sunset` headers on all v1 responses to signal
/// that clients should migrate to v2.
pub async fn deprecation_notice(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(
        "Deprecation",
        HeaderValue::from_static("true"),
    );
    headers.insert(
        "Link",
        HeaderValue::from_static(r#"</api/v2>; rel="successor-version""#),
    );
    headers.insert(
        "Sunset",
        HeaderValue::from_static("Sat, 01 Jan 2027 00:00:00 GMT"),
    );
    response
}
