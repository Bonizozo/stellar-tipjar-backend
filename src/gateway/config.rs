use std::collections::HashSet;

/// Top-level gateway configuration, loaded from environment variables.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Paths that bypass JWT/API-key authentication entirely.
    pub public_paths: HashSet<String>,
    /// Maximum request body size in bytes (default 1 MiB).
    pub max_body_bytes: usize,
    /// Whether to wrap every response in a standard envelope.
    pub response_envelope_enabled: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        let public_paths = [
            "/api/v1/creators",
            "/api/v2/creators",
            "/api/v1/tips",
            "/api/v2/tips",
            "/api/v1/health",
            "/api/v2/health",
            "/api/v1/leaderboard",
            "/api/v2/leaderboard",
            "/api/v1/stats",
            "/api/v2/stats",
            "/api/v1/auth/login",
            "/api/v2/auth/login",
            "/api/v1/auth/register",
            "/api/v2/auth/register",
            "/api/v1/auth/refresh",
            "/api/v2/auth/refresh",
            "/metrics",
            "/swagger-ui",
            "/api-docs",
            "/ws",
            "/graphql",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let max_body_bytes = std::env::var("GATEWAY_MAX_BODY_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_048_576); // 1 MiB

        let response_envelope_enabled = std::env::var("GATEWAY_RESPONSE_ENVELOPE")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        Self {
            public_paths,
            max_body_bytes,
            response_envelope_enabled,
        }
    }
}

impl GatewayConfig {
    /// Returns `true` if the path is publicly accessible without authentication.
    pub fn is_public(&self, path: &str) -> bool {
        // Exact match
        if self.public_paths.contains(path) {
            return true;
        }
        // Prefix match for parameterised public paths
        // e.g. /api/v1/creators/alice  →  public (read-only creator profile)
        let public_prefixes = [
            "/api/v1/creators/",
            "/api/v2/creators/",
            "/api/v1/health",
            "/api/v2/health",
            "/api/v1/leaderboard",
            "/api/v2/leaderboard",
            "/api/v1/stats",
            "/api/v2/stats",
            "/api/v1/search",
            "/api/v2/search",
            "/metrics",
            "/swagger-ui",
            "/api-docs",
        ];
        public_prefixes.iter().any(|prefix| path.starts_with(prefix))
    }
}
