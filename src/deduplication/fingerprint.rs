use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestFingerprint {
    pub hash: String,
    pub method: String,
    pub path: String,
    pub body_hash: String,
    pub headers_hash: String,
    pub query_params_hash: String,
    pub client_id: Option<String>,
    pub idempotency_key: Option<String>,
}

impl RequestFingerprint {
    pub fn new(
        method: &str,
        path: &str,
        body: &str,
        headers: &HashMap<String, String>,
        query_params: &HashMap<String, String>,
        client_id: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Self {
        let body_hash = Self::hash_string(body);
        let headers_hash = Self::hash_map(headers);
        let query_params_hash = Self::hash_map(query_params);

        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(path.as_bytes());
        hasher.update(body_hash.as_bytes());
        hasher.update(headers_hash.as_bytes());
        hasher.update(query_params_hash.as_bytes());
        
        if let Some(id) = client_id {
            hasher.update(id.as_bytes());
        }
        
        if let Some(key) = idempotency_key {
            hasher.update(key.as_bytes());
        }

        let hash = format!("{:x}", hasher.finalize());

        Self {
            hash,
            method: method.to_string(),
            path: path.to_string(),
            body_hash,
            headers_hash,
            query_params_hash,
            client_id: client_id.map(|s| s.to_string()),
            idempotency_key: idempotency_key.map(|s| s.to_string()),
        }
    }

    pub fn with_idempotency_key_only(
        method: &str,
        path: &str,
        idempotency_key: &str,
        client_id: Option<&str>,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(path.as_bytes());
        hasher.update(idempotency_key.as_bytes());
        
        if let Some(id) = client_id {
            hasher.update(id.as_bytes());
        }

        let hash = format!("{:x}", hasher.finalize());

        Self {
            hash,
            method: method.to_string(),
            path: path.to_string(),
            body_hash: String::new(),
            headers_hash: String::new(),
            query_params_hash: String::new(),
            client_id: client_id.map(|s| s.to_string()),
            idempotency_key: Some(idempotency_key.to_string()),
        }
    }

    fn hash_string(data: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn hash_map(map: &HashMap<String, String>) -> String {
        let mut hasher = Sha256::new();
        
        // Sort keys to ensure consistent hashing
        let mut sorted_keys: Vec<_> = map.keys().collect();
        sorted_keys.sort();
        
        for key in sorted_keys {
            if let Some(value) = map.get(key) {
                hasher.update(key.as_bytes());
                hasher.update(value.as_bytes());
            }
        }
        
        format!("{:x}", hasher.finalize())
    }

    pub fn is_idempotent_request(&self) -> bool {
        self.idempotency_key.is_some()
    }

    pub fn is_same_request(&self, other: &RequestFingerprint) -> bool {
        self.hash == other.hash
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfig {
    pub include_body: bool,
    pub include_headers: bool,
    pub include_query_params: bool,
    pub include_client_id: bool,
    pub case_sensitive: bool,
    pub normalize_paths: bool,
    pub ignore_headers: Vec<String>,
    pub ignore_query_params: Vec<String>,
}

impl Default for FingerprintConfig {
    fn default() -> Self {
        Self {
            include_body: true,
            include_headers: true,
            include_query_params: true,
            include_client_id: true,
            case_sensitive: false,
            normalize_paths: true,
            ignore_headers: vec![
                "authorization".to_string(),
                "x-request-id".to_string(),
                "x-forwarded-for".to_string(),
                "user-agent".to_string(),
            ],
            ignore_query_params: vec![
                "timestamp".to_string(),
                "nonce".to_string(),
                "_".to_string(),
            ],
        }
    }
}

pub struct FingerprintGenerator {
    config: FingerprintConfig,
}

impl FingerprintGenerator {
    pub fn new(config: FingerprintConfig) -> Self {
        Self { config }
    }

    pub fn generate_fingerprint(
        &self,
        method: &str,
        path: &str,
        body: &str,
        headers: &HashMap<String, String>,
        query_params: &HashMap<String, String>,
        client_id: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> RequestFingerprint {
        let normalized_method = if self.config.case_sensitive {
            method.to_string()
        } else {
            method.to_lowercase()
        };

        let normalized_path = if self.config.normalize_paths {
            self.normalize_path(path)
        } else if self.config.case_sensitive {
            path.to_string()
        } else {
            path.to_lowercase()
        };

        let normalized_body = if self.config.case_sensitive {
            body.to_string()
        } else {
            body.to_lowercase()
        };

        let filtered_headers = self.filter_headers(headers);
        let filtered_query_params = self.filter_query_params(query_params);

        if let Some(key) = idempotency_key {
            // If idempotency key is provided, use it as primary identifier
            RequestFingerprint::with_idempotency_key_only(
                &normalized_method,
                &normalized_path,
                key,
                client_id,
            )
        } else {
            RequestFingerprint::new(
                &normalized_method,
                &normalized_path,
                &normalized_body,
                &filtered_headers,
                &filtered_query_params,
                client_id,
                None,
            )
        }
    }

    fn normalize_path(&self, path: &str) -> String {
        // Remove trailing slashes
        let mut normalized = path.trim_end_matches('/');
        
        // Convert to lowercase if not case sensitive
        if !self.config.case_sensitive {
            normalized = normalized.to_lowercase().as_str();
        }
        
        // Remove multiple consecutive slashes
        while normalized.contains("//") {
            normalized = normalized.replace("//", "/");
        }
        
        normalized.to_string()
    }

    fn filter_headers(&self, headers: &HashMap<String, String>) -> HashMap<String, String> {
        let mut filtered = HashMap::new();
        
        for (key, value) in headers {
            let normalized_key = if self.config.case_sensitive {
                key.clone()
            } else {
                key.to_lowercase()
            };

            // Skip ignored headers
            if self.config.ignore_headers.contains(&normalized_key) {
                continue;
            }

            let normalized_value = if self.config.case_sensitive {
                value.clone()
            } else {
                value.to_lowercase()
            };

            filtered.insert(normalized_key, normalized_value);
        }
        
        filtered
    }

    fn filter_query_params(&self, query_params: &HashMap<String, String>) -> HashMap<String, String> {
        let mut filtered = HashMap::new();
        
        for (key, value) in query_params {
            let normalized_key = if self.config.case_sensitive {
                key.clone()
            } else {
                key.to_lowercase()
            };

            // Skip ignored query params
            if self.config.ignore_query_params.contains(&normalized_key) {
                continue;
            }

            let normalized_value = if self.config.case_sensitive {
                value.clone()
            } else {
                value.to_lowercase()
            };

            filtered.insert(normalized_key, normalized_value);
        }
        
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_fingerprint_creation() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        
        let mut query_params = HashMap::new();
        query_params.insert("page".to_string(), "1".to_string());

        let fingerprint = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &headers,
            &query_params,
            Some("client123"),
            None,
        );

        assert_eq!(fingerprint.method, "POST");
        assert_eq!(fingerprint.path, "/api/v1/tips");
        assert_eq!(fingerprint.client_id, Some("client123".to_string()));
        assert!(!fingerprint.hash.is_empty());
    }

    #[test]
    fn test_fingerprint_with_idempotency_key() {
        let fingerprint = RequestFingerprint::with_idempotency_key_only(
            "POST",
            "/api/v1/tips",
            "idemp123",
            Some("client123"),
        );

        assert!(fingerprint.is_idempotent_request());
        assert_eq!(fingerprint.idempotency_key, Some("idemp123".to_string()));
    }

    #[test]
    fn test_fingerprint_generator() {
        let config = FingerprintConfig::default();
        let generator = FingerprintGenerator::new(config);

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("authorization".to_string(), "Bearer token123".to_string());
        
        let mut query_params = HashMap::new();
        query_params.insert("page".to_string(), "1".to_string());
        query_params.insert("timestamp".to_string(), "123456789".to_string());

        let fingerprint = generator.generate_fingerprint(
            "POST",
            "/api/v1/tips/",
            "{\"amount\":100}",
            &headers,
            &query_params,
            Some("client123"),
            None,
        );

        // Should normalize path (remove trailing slash)
        assert_eq!(fingerprint.path, "/api/v1/tips");
        
        // Should filter out ignored headers and query params
        assert!(!fingerprint.headers_hash.is_empty());
        assert!(!fingerprint.query_params_hash.is_empty());
    }

    #[test]
    fn test_fingerprint_consistency() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        
        let fingerprint1 = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &headers,
            &HashMap::new(),
            None,
            None,
        );

        let fingerprint2 = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &headers,
            &HashMap::new(),
            None,
            None,
        );

        assert_eq!(fingerprint1.hash, fingerprint2.hash);
        assert!(fingerprint1.is_same_request(&fingerprint2));
    }

    #[test]
    fn test_fingerprint_difference() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        
        let fingerprint1 = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &headers,
            &HashMap::new(),
            None,
            None,
        );

        let fingerprint2 = RequestFingerprint::new(
            "POST",
            "/api/v1/tips",
            "{\"amount\":200}",
            &headers,
            &HashMap::new(),
            None,
            None,
        );

        assert_ne!(fingerprint1.hash, fingerprint2.hash);
        assert!(!fingerprint1.is_same_request(&fingerprint2));
    }
}
