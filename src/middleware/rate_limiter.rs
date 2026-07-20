use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::PeerIpKeyExtractor, GovernorLayer,
};
use governor::middleware::StateInformationMiddleware;

use crate::config::RateLimitConfig;

/// Builds a [`GovernorLayer`] for general read endpoints using values from
/// [`RateLimitConfig`].
pub fn general_limiter(
    cfg: &RateLimitConfig,
) -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    build_layer(cfg.general_per_second, cfg.general_burst_size)
}

/// Builds a stricter [`GovernorLayer`] for write endpoints.
pub fn write_limiter(
    cfg: &RateLimitConfig,
) -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    build_layer(cfg.write_per_second, cfg.write_burst_size)
}

fn build_layer(
    per_second: u64,
    burst_size: u32,
) -> GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware> {
    // SAFETY: GovernorConfigBuilder::finish() only returns None when
    // burst_size == 0.  Both defaults and user-supplied values are
    // validated as > 0 by AppConfig. Invariant: burst_size >= 1.
    #[allow(clippy::expect_used)]
    let governor_cfg = GovernorConfigBuilder::default()
        .per_second(per_second)
        .burst_size(burst_size)
        .use_headers()
        .finish()
        .expect("GovernorConfig::finish invariant: burst_size >= 1");
    let config = Arc::new(governor_cfg);

    let limiter = config.limiter().clone();
    // Spawn a cleanup task to prune stale IP entries every 60 seconds.
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

// ---------------------------------------------------------------------------
// Whitelist middleware
// ---------------------------------------------------------------------------

/// Axum middleware that bypasses rate limiting for whitelisted IPs.
///
/// The whitelist is read from [`AppState::config.rate_limit.whitelist`] — it is
/// resolved once at startup rather than per-request.
pub async fn whitelist_middleware(req: Request, next: Next) -> Response {
    // The whitelist is injected as an `axum::Extension` by main.rs so this
    // middleware has access to it without an additional state parameter.
    let ip = req
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip());

    if let Some(ip) = ip {
        if let Some(whitelist) = req.extensions().get::<Arc<Vec<IpAddr>>>() {
            if whitelist.iter().any(|&allowed| allowed == ip) {
                tracing::debug!(%ip, "whitelisted IP bypassing rate limit");
                return next.run(req).await;
            }
        }
    }
    next.run(req).await
}
