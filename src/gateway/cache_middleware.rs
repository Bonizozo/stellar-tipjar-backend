use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cache::{CachedHttpResponse, MultiLayerCache};
use crate::errors::AppError;

/// Configuration for gateway caching middleware
#[derive(Clone)]
pub struct GatewayCacheConfig {
    /// Default TTL for cached responses
    pub default_ttl: Duration,
    /// Maximum TTL allowed
    pub max_ttl: Duration,
    /// Cacheable HTTP methods
    pub cacheable_methods: Vec<String>,
    /// Cacheable status codes
    pub cacheable_status_codes: Vec<u16>,
    /// Cache key generation strategy
    pub key_strategy: CacheKeyStrategy,
    /// Cache warming configuration
    pub warming_config: Option<CacheWarmingConfig>,
}

impl Default for GatewayCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_ttl: Duration::from_secs(3600),    // 1 hour
            cacheable_methods: vec!["GET".to_string(), "HEAD".to_string()],
            cacheable_status_codes: vec![200, 201, 301, 302, 304, 404],
            key_strategy: CacheKeyStrategy::default(),
            warming_config: None,
        }
    }
}

/// Cache key generation strategies
#[derive(Clone, Debug)]
pub enum CacheKeyStrategy {
    /// Simple path-based key
    Path,
    /// Path + query parameters
    PathQuery,
    /// Path + query + selected headers
    PathQueryHeaders(Vec<String>),
    /// Custom key generation function
    Custom(fn(&Request) -> String),
}

impl Default for CacheKeyStrategy {
    fn default() -> Self {
        Self::PathQuery
    }
}

/// Cache warming configuration
#[derive(Clone)]
pub struct CacheWarmingConfig {
    /// Endpoints to warm
    pub endpoints: Vec<String>,
    /// Warming interval
    pub interval: Duration,
    /// Concurrent warming requests
    pub concurrent_requests: usize,
}

/// Cache metrics for monitoring
#[derive(Clone, Debug, Default)]
pub struct CacheMetrics {
    pub hits: Arc<std::sync::atomic::AtomicU64>,
    pub misses: Arc<std::sync::atomic::AtomicU64>,
    pub sets: Arc<std::sync::atomic::AtomicU64>,
    pub invalidations: Arc<std::sync::atomic::AtomicU64>,
    pub warm_requests: Arc<std::sync::atomic::AtomicU64>,
}

impl CacheMetrics {
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(std::sync::atomic::Ordering::Relaxed);
        let misses = self.misses.load(std::sync::atomic::Ordering::Relaxed);
        if hits + misses == 0 {
            0.0
        } else {
            hits as f64 / (hits + misses) as f64
        }
    }

    pub fn increment_hits(&self) {
        self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_misses(&self) {
        self.misses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_sets(&self) {
        self.sets.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_invalidations(&self) {
        self.invalidations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_warm_requests(&self) {
        self.warm_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Gateway cache state
#[derive(Clone)]
pub struct GatewayCacheState {
    pub cache: Arc<MultiLayerCache>,
    pub config: GatewayCacheConfig,
    pub metrics: CacheMetrics,
}

impl GatewayCacheState {
    pub fn new(cache: Arc<MultiLayerCache>, config: GatewayCacheConfig) -> Self {
        Self {
            cache,
            config,
            metrics: CacheMetrics::default(),
        }
    }
}

/// Generate cache key based on strategy
fn generate_cache_key(req: &Request, strategy: &CacheKeyStrategy) -> String {
    match strategy {
        CacheKeyStrategy::Path => {
            format!("gateway:{}", req.uri().path())
        }
        CacheKeyStrategy::PathQuery => {
            format!(
                "gateway:{}{}",
                req.uri().path(),
                req.uri().query().unwrap_or("")
            )
        }
        CacheKeyStrategy::PathQueryHeaders(headers) => {
            let mut key = format!(
                "gateway:{}{}",
                req.uri().path(),
                req.uri().query().unwrap_or("")
            );
            
            for header_name in headers {
                if let Some(value) = req.headers().get(header_name) {
                    if let Ok(value_str) = value.to_str() {
                        key.push(':');
                        key.push_str(value_str);
                    }
                }
            }
            
            key
        }
        CacheKeyStrategy::Custom(func) => func(req),
    }
}

/// Check if response should be cached
fn should_cache_response(
    status: StatusCode,
    headers: &axum::http::HeaderMap,
    config: &GatewayCacheConfig,
) -> bool {
    // Check status code
    if !config.cacheable_status_codes.contains(&status.as_u16()) {
        return false;
    }

    // Check cache control headers
    if let Some(cache_control) = headers.get(header::CACHE_CONTROL) {
        if let Ok(cache_control_str) = cache_control.to_str() {
            if cache_control_str.contains("no-cache") || cache_control_str.contains("private") {
                return false;
            }
        }
    }

    // Check authorization header - don't cache authenticated responses
    if headers.get(header::AUTHORIZATION).is_some() {
        return false;
    }

    true
}

/// Extract TTL from response headers or use default
fn extract_cache_ttl(headers: &axum::http::HeaderMap, default_ttl: Duration, max_ttl: Duration) -> Duration {
    if let Some(cache_control) = headers.get(header::CACHE_CONTROL) {
        if let Ok(cache_control_str) = cache_control.to_str() {
            // Parse max-age directive
            for directive in cache_control_str.split(',') {
                let directive = directive.trim();
                if directive.starts_with("max-age=") {
                    if let Ok(seconds) = directive[8..].parse::<u64>() {
                        let ttl = Duration::from_secs(seconds);
                        return std::cmp::min(ttl, max_ttl);
                    }
                }
            }
        }
    }
    default_ttl
}

/// API Gateway caching middleware
pub async fn gateway_cache_middleware(
    State(state): State<Arc<GatewayCacheState>>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    // Check if method is cacheable
    let method = req.method().as_str();
    if !state.config.cacheable_methods.contains(&method.to_string()) {
        return Ok(next.run(req).await);
    }

    // Generate cache key
    let cache_key = generate_cache_key(&req, &state.config.key_strategy);

    // Try to get from cache
    if let Ok(Some(cached_response)) = state.cache.get_http_response(&cache_key).await {
        state.metrics.increment_hits();
        
        let mut response = cached_response.status().into_response();
        
        // Copy cached headers
        for (name, values) in &cached_response.headers {
            for value in values {
                if let Ok(header_value) = HeaderValue::from_str(value) {
                    response.headers_mut().append(name, header_value);
                }
            }
        }
        
        // Add cache headers
        response.headers_mut().insert(
            header::X_CACHE,
            HeaderValue::from_static("HIT"),
        );
        
        response.headers_mut().insert(
            header::AGE,
            HeaderValue::from_str(
                &chrono::Utc::now()
                    .signed_duration_since(cached_response.cached_at)
                    .num_seconds()
                    .to_string()
            ).unwrap_or(HeaderValue::from_static("0")),
        );
        
        return Ok(response);
    }

    state.metrics.increment_misses();

    // Cache miss - proceed with request
    let mut response = next.run(req).await;

    // Check if response should be cached
    if should_cache_response(response.status(), response.headers(), &state.config) {
        // Extract TTL
        let ttl = extract_cache_ttl(
            response.headers(),
            state.config.default_ttl,
            state.config.max_ttl,
        );

        // Collect headers
        let mut headers_map = HashMap::new();
        for (name, value) in response.headers() {
            let name_str = name.to_string();
            let value_str = value.to_str().unwrap_or("");
            headers_map.entry(name_str).or_insert_with(Vec::new).push(value_str.to_string());
        }

        // Get response body
        let (parts, body) = response.into_parts();
        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();

        // Create cached response
        let cached_response = CachedHttpResponse::new(
            parts.status,
            headers_map,
            body_bytes.to_vec(),
        );

        // Store in cache
        if let Err(e) = state.cache.set_http_response(&cache_key, &cached_response, ttl).await {
            tracing::warn!("Failed to cache response: {}", e);
        } else {
            state.metrics.increment_sets();
        }

        // Reconstruct response
        response = Response::from_parts(parts, axum::body::Body::from(body_bytes));

        // Add cache headers
        response.headers_mut().insert(
            header::X_CACHE,
            HeaderValue::from_static("MISS"),
        );
    } else {
        response.headers_mut().insert(
            header::X_CACHE,
            HeaderValue::from_static("BYPASS"),
        );
    }

    Ok(response)
}

/// Cache invalidation middleware for write operations
pub async fn cache_invalidation_middleware(
    State(state): State<Arc<GatewayCacheState>>,
    req: Request,
    next: Next,
) -> Response {
    let response = next.run(req).await;

    // Invalidate cache on successful write operations
    if response.status().is_success() {
        let path = req.uri().path();
        
        // Invalidate patterns based on the path
        let patterns = generate_invalidation_patterns(path);
        
        for pattern in patterns {
            if let Err(e) = state.cache.invalidate_pattern(&pattern).await {
                tracing::warn!("Failed to invalidate cache pattern '{}': {}", pattern, e);
            } else {
                state.metrics.increment_invalidations();
            }
        }
    }

    response
}

/// Generate cache invalidation patterns based on request path
fn generate_invalidation_patterns(path: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    
    // Invalidate exact path
    patterns.push(format!("gateway:{}", path));
    
    // Invalidate parent collections
    let path_parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if path_parts.len() > 1 {
        patterns.push(format!("gateway:/{}", path_parts[..path_parts.len()-1].join("/")));
    }
    
    // Invalidate list endpoints
    if path.contains("/creators/") {
        patterns.push("gateway:/api/v*/creators".to_string());
    }
    if path.contains("/tips/") {
        patterns.push("gateway:/api/v*/tips".to_string());
    }
    
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Uri};

    #[test]
    fn test_cache_key_generation() {
        let req = Request::builder()
            .method(Method::GET)
            .uri(Uri::from_static("/api/v1/creators?sort=name"))
            .body(Body::empty())
            .unwrap();

        let key = generate_cache_key(&req, &CacheKeyStrategy::Path);
        assert_eq!(key, "gateway:/api/v1/creators");

        let key = generate_cache_key(&req, &CacheKeyStrategy::PathQuery);
        assert_eq!(key, "gateway:/api/v1/creators?sort=name");
    }

    #[test]
    fn test_invalidation_patterns() {
        let patterns = generate_invalidation_patterns("/api/v1/creators/alice");
        assert!(patterns.contains(&"gateway:/api/v1/creators/alice".to_string()));
        assert!(patterns.contains(&"gateway:/api/v1/creators".to_string()));
        assert!(patterns.contains(&"gateway:/api/v*/creators".to_string()));
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = CacheMetrics::default();
        metrics.increment_hits();
        metrics.increment_hits();
        metrics.increment_misses();
        
        assert_eq!(metrics.hit_rate(), 2.0 / 3.0);
    }
}
