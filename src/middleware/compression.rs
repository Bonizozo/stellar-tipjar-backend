use tower_http::compression::CompressionLayer;
use tower_http::CompressionLevel;
use tower_http::compression::predicate::{Predicate, SizeAbove, NotForContentType};

/// Returns a compression layer configured with:
/// - Support for gzip, brotli, and deflate.
/// - Default compression quality.
/// - Minimum size threshold of 1KB (1024 bytes).
/// - Excludes image content types from compression.
pub fn compression_layer() -> CompressionLayer<impl Predicate + Clone> {
    let predicate = SizeAbove::new(1024)      // Only compress responses > 1KB
        .and(NotForContentType::IMAGES); // Don't compress images

    CompressionLayer::new()
        .gzip(true)
        .br(true)
        .deflate(true)
        .quality(CompressionLevel::Default)
        .compress_when(predicate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use axum::{Router, response::IntoResponse, routing::get, http::header};

    #[tokio::test]
    async fn test_compression_logic() {
        let app = Router::new()
            .route("/large", get(|| async { "a".repeat(1025) }))
            .route("/small", get(|| async { "a".repeat(100) }))
            .route("/image", get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/png")],
                    "a".repeat(2000),
                )
            }))
            .layer(compression_layer());

        let server = TestServer::new(app).unwrap();

        // 1. Large string should be compressed
        let response = server
            .get("/large")
            .add_header(header::ACCEPT_ENCODING, "gzip")
            .await;
        assert_eq!(response.header(header::CONTENT_ENCODING).to_str().unwrap(), "gzip");

        // 2. Small string should NOT be compressed (threshold 1024)
        let response = server
            .get("/small")
            .add_header(header::ACCEPT_ENCODING, "gzip")
            .await;
        assert!(response.maybe_header(header::CONTENT_ENCODING).is_none());

        // 3. Image should NOT be compressed
        let response = server
            .get("/image")
            .add_header(header::ACCEPT_ENCODING, "gzip")
            .await;
        assert!(response.maybe_header(header::CONTENT_ENCODING).is_none());

        // 4. Brotli support
        let response = server
            .get("/large")
            .add_header(header::ACCEPT_ENCODING, "br")
            .await;
        assert_eq!(response.header(header::CONTENT_ENCODING).to_str().unwrap(), "br");
    }
}
