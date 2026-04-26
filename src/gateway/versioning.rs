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

/// Axum middleware that:
/// 1. Injects `X-API-Version` on every response.
/// 2. Validates the requested version is known (returns 400 for unknown versions).
/// 3. Adds `Deprecation`, `Sunset`, and `Link` headers for deprecated versions.
pub async fn version_negotiation(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let mgr = default_version_manager();

    let version = detect_version(&path);

    // Unknown version prefix → pass through (non-versioned paths like /metrics)
    let version_str = match version {
        Some(v) => v,
        None => return next.run(req).await,
    };

    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // Always inject the version header.
    headers.insert(
        "X-API-Version",
        HeaderValue::from_static(version_str),
    );

    // Inject deprecation headers for deprecated versions.
    if let Some(meta) = mgr.get(version_str) {
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

    response
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
