use crate::models::auth::Claims;
use uuid::Uuid;

/// The authenticated identity attached to a request by the gateway auth middleware.
///
/// Injected into `Request::extensions()` so downstream handlers can read it
/// without re-validating credentials.
#[derive(Debug, Clone)]
pub enum GatewayIdentity {
    /// Request authenticated via a Bearer JWT.
    Jwt {
        subject: String,
        role: String,
        claims: Claims,
    },
    /// Request authenticated via X-API-Key + X-API-Secret.
    ApiKey {
        key_id: Uuid,
        key: String,
        name: String,
        permissions: Vec<String>,
    },
    /// Unauthenticated request on a public endpoint.
    Anonymous,
}

impl GatewayIdentity {
    /// Returns `true` if the identity has the given permission string.
    ///
    /// JWT identities are granted all permissions (role-based checks happen
    /// downstream in `authorization` middleware).  API key identities are
    /// checked against their explicit permission list.
    pub fn has_permission(&self, permission: &str) -> bool {
        match self {
            Self::Jwt { .. } => true,
            Self::ApiKey { permissions, .. } => {
                permissions.iter().any(|p| p == permission || p == "*")
            }
            Self::Anonymous => false,
        }
    }

    /// Returns a loggable string identifying the caller.
    pub fn display(&self) -> String {
        match self {
            Self::Jwt { subject, role, .. } => format!("jwt:{}:{}", role, subject),
            Self::ApiKey { key, name, .. } => format!("apikey:{}:{}", name, &key[..8]),
            Self::Anonymous => "anonymous".to_string(),
        }
    }
}
