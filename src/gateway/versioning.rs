use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};
use std::collections::HashMap;

// ── Version registry ──────────────────────────────────────────────────────────

/// Metadata for a single API version.
#[derive(Debug, Clone)]
pub struct ApiVersion {
    pub version: String,
    pub deprecated: bool,
    /// RFC 7231 HTTP-date string, e.g. `"Sat, 01 Jan 2027 00:00:00 GMT"`.
    pub sunset_date: Option<String>,
    /// URL to the migration guide for this version.
    pub migration_url: Option<String>,
}

/// Registry of all known API versions.
#[derive(Debug, Clone)]
pub struct ApiVersionManager {
    versions: HashMap<String, ApiVersion>,
    current_version: String,
}

impl ApiVersionManager {
    pub fn new(current_version: impl Into<String>) -> Self {
        Self {
            versions: HashMap::new(),
            current_version: current_version.into(),
        }
    }

    pub fn register(
        &mut self,
        version: impl Into<String>,
        deprecated: bool,
        sunset_date: Option<String>,
        migration_url: Option<String>,
    ) {
        let v = version.into();
        self.versions.insert(
            v.clone(),
            ApiVersion {
                version: v,
                deprecated,
                sunset_date,
                migration_url,
            },
        );
    }

    pub fn get(&self, version: &str) -> Option<&ApiVersion> {
        self.versions.get(version)
    }

    pub fn is_supported(&self, version: &str) -> bool {
        self.versions
            .get(version)
            .map(|v| !v.deprecated)
            .unwrap_or(false)
    }

    pub fn current(&self) -> &str {
        &self.current_version
    }

    pub fn deprecated_versions(&self) -> Vec<&ApiVersion> {
        self.versions.values().filter(|v| v.deprecated).collect()
    }
}

/// Build the default version manager for this project.
pub fn default_version_manager() -> ApiVersionManager {
    let mut mgr = ApiVersionManager::new("v2");
    mgr.register(
        "v1",
        true,
        Some("Sat, 01 Jan 2027 00:00:00 GMT".to_string()),
        Some("https://docs.example.com/migration/v1-to-v2".to_string()),
    );
    mgr.register("v2", false, None, None);
    mgr
}

// ── Axum middleware ───────────────────────────────────────────────────────────

/// Detect the API version from the request path.
fn detect_version(path: &str) -> Option<&'static str> {
    if path.contains("/v1/") || path.ends_with("/v1") {
        Some("v1")
    } else if path.contains("/v2/") || path.ends_with("/v2") {
        Some("v2")
    } else {
        None
    }
}

/// Detect API version from Accept header (content negotiation)
fn detect_version_from_accept(accept_header: &str) -> Option<&'static str> {
    // Parse Accept header for version information
    // Example: "application/json; version=v1" or "application/vnd.api.v1+json"
    if accept_header.contains("version=v1") || accept_header.contains("vnd.api.v1") {
        Some("v1")
    } else if accept_header.contains("version=v2") || accept_header.contains("vnd.api.v2") {
        Some("v2")
    } else {
        None
    }
}

/// Detect API version from custom header
fn detect_version_from_header(headers: &axum::http::HeaderMap) -> Option<&'static str> {
    if let Some(header_value) = headers.get("X-API-Version") {
        if let Ok(version_str) = header_value.to_str() {
            match version_str {
                "v1" => return Some("v1"),
                "v2" => return Some("v2"),
                _ => {}
            }
        }
    }
    
    if let Some(header_value) = headers.get("API-Version") {
        if let Ok(version_str) = header_value.to_str() {
            match version_str {
                "v1" => return Some("v1"),
                "v2" => return Some("v2"),
                _ => {}
            }
        }
    }
    
    None
}

/// Resolve API version using multiple strategies in priority order
fn resolve_api_version(req: &axum::extract::Request) -> Option<VersionResolution> {
    let path = req.uri().path();
    
    // Priority 1: Path-based versioning (most explicit)
    if let Some(version) = detect_version(path) {
        return Some(VersionResolution {
            version: version.to_string(),
            source: VersionSource::Path,
            confidence: 1.0,
        });
    }
    
    // Priority 2: Custom headers
    if let Some(version) = detect_version_from_header(req.headers()) {
        return Some(VersionResolution {
            version: version.to_string(),
            source: VersionSource::Header,
            confidence: 0.9,
        });
    }
    
    // Priority 3: Accept header (content negotiation)
    if let Some(accept_header) = req.headers().get(axum::http::header::ACCEPT) {
        if let Ok(accept_str) = accept_header.to_str() {
            if let Some(version) = detect_version_from_accept(accept_str) {
                return Some(VersionResolution {
                    version: version.to_string(),
                    source: VersionSource::AcceptHeader,
                    confidence: 0.8,
                });
            }
        }
    }
    
    // Priority 4: Default version (if no version specified)
    None
}

/// Version resolution information
#[derive(Debug, Clone)]
pub struct VersionResolution {
    pub version: String,
    pub source: VersionSource,
    pub confidence: f64,
}

/// Version detection method
#[derive(Debug, Clone, PartialEq)]
pub enum VersionSource {
    Path,
    Header,
    AcceptHeader,
    Default,
}

/// Enhanced version negotiation middleware supporting multiple version detection strategies
pub async fn version_negotiation(req: Request, next: Next) -> Response {
    let mgr = default_version_manager();
    let path = req.uri().path();

    // Resolve version using multiple strategies
    let version_resolution = resolve_api_version(&req);
    
    // For non-versioned paths (like /metrics), pass through
    if version_resolution.is_none() && !path.starts_with("/api/") {
        return next.run(req).await;
    }

    // Use resolved version or default for API paths
    let version_str = version_resolution
        .map(|r| r.version)
        .unwrap_or_else(|| mgr.current().to_string());

    // Validate version exists
    if !mgr.versions.contains_key(&version_str) {
        let mut response = axum::http::Response::builder()
            .status(axum::http::StatusCode::BAD_REQUEST)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"error": "Unsupported API version '{}', supported versions: {}"#,
                version_str,
                mgr.versions.keys().cloned().collect::<Vec<_>>().join(", ")
            )))
            .unwrap();
        
        // Add supported versions header
        response.headers_mut().insert(
            "X-API-Versions",
            HeaderValue::from_str(
                &mgr.versions.keys().cloned().collect::<Vec<_>>().join(", ")
            ).unwrap_or(HeaderValue::from_static("v1,v2")),
        );
        
        return response;
    }

    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // Always inject the version header
    headers.insert(
        "X-API-Version",
        HeaderValue::from_str(&version_str).unwrap_or(HeaderValue::from_static("unknown")),
    );

    // Add version detection method header for debugging
    if let Some(resolution) = version_resolution {
        headers.insert(
            "X-Version-Detection-Method",
            HeaderValue::from_str(match resolution.source {
                VersionSource::Path => "path",
                VersionSource::Header => "header",
                VersionSource::AcceptHeader => "accept",
                VersionSource::Default => "default",
            }).unwrap_or(HeaderValue::from_static("unknown")),
        );
    }

    // Add Vary header to inform clients about version negotiation
    headers.insert(
        axum::http::header::VARY,
        HeaderValue::from_static("Accept, X-API-Version, API-Version"),
    );

    // Inject deprecation headers for deprecated versions
    if let Some(meta) = mgr.get(&version_str) {
        if meta.deprecated {
            headers.insert("Deprecation", HeaderValue::from_static("true"));

            if let Some(ref sunset) = meta.sunset_date {
                if let Ok(v) = HeaderValue::from_str(sunset) {
                    headers.insert("Sunset", v);
                }
            }

            if let Some(ref url) = meta.migration_url {
                let link = format!("<{}>; rel=\"successor-version\"", url);
                if let Ok(v) = HeaderValue::from_str(&link) {
                    headers.insert("Link", v);
                }
            }

            headers.insert(
                "X-Deprecation-Warning",
                HeaderValue::from_static(
                    "This API version is deprecated. Please migrate to /api/v2",
                ),
            );
        }
    }

    // Add API documentation links
    let docs_link = format!("<https://docs.example.com/api/{}>; rel=\"api-documentation\"", version_str);
    if let Ok(v) = HeaderValue::from_str(&docs_link) {
        headers.insert("Link", v);
    }

    response
}

/// Middleware to handle version routing for non-versioned endpoints
pub async fn version_routing(req: Request, next: Next) -> Response {
    let mgr = default_version_manager();
    let path = req.uri().path();
    
    // Only apply to API endpoints without explicit version
    if path.starts_with("/api/") && !path.contains("/v1/") && !path.contains("/v2/") {
        let resolution = resolve_api_version(&req);
        let target_version = resolution
            .map(|r| r.version)
            .unwrap_or_else(|| mgr.current().to_string());
        
        // Add version context to request extensions for downstream handlers
        let mut req = req;
        req.extensions_mut().insert(ApiVersionContext {
            version: target_version.clone(),
            source: resolution.map(|r| r.source).unwrap_or(VersionSource::Default),
        });
        
        return next.run(req).await;
    }
    
    next.run(req).await
}

/// Version context attached to requests
#[derive(Debug, Clone)]
pub struct ApiVersionContext {
    pub version: String,
    pub source: VersionSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_detection() {
        assert_eq!(detect_version("/api/v1/creators"), Some("v1"));
        assert_eq!(detect_version("/api/v2/tips"), Some("v2"));
        assert_eq!(detect_version("/metrics"), None);
        assert_eq!(detect_version("/ws"), None);
    }

    #[test]
    fn version_manager_deprecation() {
        let mgr = default_version_manager();
        assert!(!mgr.is_supported("v1"));
        assert!(mgr.is_supported("v2"));
        assert_eq!(mgr.current(), "v2");
        assert_eq!(mgr.deprecated_versions().len(), 1);
    }
}
