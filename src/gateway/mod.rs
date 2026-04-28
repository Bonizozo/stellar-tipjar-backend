pub mod api_gateway;
pub mod authentication;
pub mod cache_middleware;
pub mod config;
pub mod context;
pub mod middleware;
pub mod request_transformer;
pub mod versioning;

pub use api_gateway::ApiGateway;
pub use authentication::gateway_auth;
pub use cache_middleware::{gateway_cache_middleware, cache_invalidation_middleware, GatewayCacheConfig, GatewayCacheState, CacheMetrics};
pub use config::GatewayConfig;
pub use context::GatewayIdentity;
pub use middleware::{gateway_metrics, inject_identity_header, propagate_request_id_to_response, require_scope};
pub use request_transformer::transform_request;
pub use versioning::{version_negotiation, version_routing, ApiVersionContext, VersionResolution, VersionSource};
