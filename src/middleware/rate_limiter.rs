use std::sync::Arc;
use std::time::Duration;
use tower_governor::{
    governor::{GovernorConfig, GovernorConfigBuilder},
    key_extractor::PeerIpKeyExtractor,
    GovernorLayer,
};
use governor::{middleware::StateInformationMiddleware, clock::QuantaInstant};

/// Builds a tuple of (config, layer) for general read endpoints.
pub fn general_limiter() -> (
    Arc<GovernorConfig<PeerIpKeyExtractor, StateInformationMiddleware>>, 
    GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware>
) {
    let per_second = env_u64("RATE_LIMIT_PER_SECOND", 10);
    let burst_size = env_u32("RATE_LIMIT_BURST_SIZE", 20);
    build_config_and_layer(per_second, burst_size)
}

/// Builds a stricter tuple of (config, layer) for write endpoints.
pub fn write_limiter() -> (
    Arc<GovernorConfig<PeerIpKeyExtractor, StateInformationMiddleware>>,
    GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware>
) {
    let per_second = env_u64("RATE_LIMIT_WRITE_PER_SECOND", 2);
    let burst_size = env_u32("RATE_LIMIT_WRITE_BURST_SIZE", 5);
    build_config_and_layer(per_second, burst_size)
}

fn build_config_and_layer(
    per_second: u64,
    burst_size: u32,
) -> (
    Arc<GovernorConfig<PeerIpKeyExtractor, StateInformationMiddleware>>,
    GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware>
) {
    let config = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(per_second)
            .burst_size(burst_size)
            .finish()
            .unwrap(),
    );

    let layer = GovernorLayer { config: config.clone() };
    (config, layer)
}

/// Helper if we manually need to spawn cleanup from config.
pub fn spawn_cleanup(config: &Arc<GovernorConfig<PeerIpKeyExtractor, StateInformationMiddleware>>) {
    let limiter = config.limiter().clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await; 
        loop {
            interval.tick().await;
            limiter.retain_recent();
        }
    });
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
