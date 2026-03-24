use std::sync::Arc;
use std::time::Duration;
use tower_governor::{
    governor::GovernorConfigBuilder,
    key_extractor::PeerIpKeyExtractor,
    GovernorLayer,
};

/// Builds a GovernorLayer for general read endpoints.
/// Configurable via env: RATE_LIMIT_PER_SECOND (default 10), RATE_LIMIT_BURST_SIZE (default 20).
pub fn general_limiter() -> GovernorLayer<PeerIpKeyExtractor, governor::middleware::NoOpMiddleware<governor::clock::QuantaInstant>> {
    let per_second = env_u64("RATE_LIMIT_PER_SECOND", 10);
    let burst_size = env_u32("RATE_LIMIT_BURST_SIZE", 20);
    build_layer(per_second, burst_size)
}

/// Builds a stricter GovernorLayer for write endpoints (POST /tips, POST /creators).
/// Configurable via env: RATE_LIMIT_WRITE_PER_SECOND (default 2), RATE_LIMIT_WRITE_BURST_SIZE (default 5).
pub fn write_limiter() -> GovernorLayer<PeerIpKeyExtractor, governor::middleware::NoOpMiddleware<governor::clock::QuantaInstant>> {
    let per_second = env_u64("RATE_LIMIT_WRITE_PER_SECOND", 2);
    let burst_size = env_u32("RATE_LIMIT_WRITE_BURST_SIZE", 5);
    build_layer(per_second, burst_size)
}

fn build_layer(
    per_second: u64,
    burst_size: u32,
) -> GovernorLayer<PeerIpKeyExtractor, governor::middleware::NoOpMiddleware<governor::clock::QuantaInstant>> {
    let config = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(per_second)
            .burst_size(burst_size)
            .use_headers()
            .finish()
            .unwrap(),
    );

    // Spawn a background task to prune stale entries every 60 seconds.
    let limiter = config.limiter().clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await; // skip immediate first tick
        loop {
            interval.tick().await;
            tracing::debug!("rate limiter cleanup: {} tracked IPs", limiter.len());
            limiter.retain_recent();
        }
    });

    GovernorLayer { config }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
