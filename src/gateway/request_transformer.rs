use axum::{
    body::{to_bytes, Body},
    extract::Request,
    middleware::Next,
    response::Response,
};
use serde_json::Value;
use std::collections::HashMap;

// ── Transformation rules ──────────────────────────────────────────────────────

/// A single transformation rule applied to requests matching `path_prefix`.
#[derive(Debug, Clone)]
pub struct TransformationRule {
    /// URL path prefix this rule applies to (e.g. `"/api/v1/tips"`).
    pub path_prefix: String,
    /// Headers to add to the forwarded request.
    pub add_headers: HashMap<String, String>,
    /// Header names to strip from the forwarded request.
    pub remove_headers: Vec<String>,
    /// Optional named body transform (see `apply_body_transform`).
    pub body_transform: Option<String>,
}

/// Registry of transformation rules, keyed by path prefix.
#[derive(Debug, Clone, Default)]
pub struct RequestTransformer {
    rules: Vec<TransformationRule>,
}

impl RequestTransformer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a transformation rule.
    pub fn add_rule(&mut self, rule: TransformationRule) {
        self.rules.push(rule);
    }

    /// Find the first rule whose `path_prefix` matches `path`.
    pub fn find_rule(&self, path: &str) -> Option<&TransformationRule> {
        self.rules
            .iter()
            .find(|r| path.starts_with(&r.path_prefix))
    }

    /// Apply named body transforms to a JSON value.
    pub fn apply_body_transform(body: &Value, transform: &str) -> Value {
        match transform {
            // Trim whitespace from the `amount` field.
            "normalize_amount" => {
                if let Some(amount) = body.get("amount").and_then(|v| v.as_str()) {
                    let mut result = body.clone();
                    result["amount"] = serde_json::json!(amount.trim());
                    result
                } else {
                    body.clone()
                }
            }
            // Lowercase the `username` field.
            "lowercase_username" => {
                if let Some(username) = body.get("username").and_then(|v| v.as_str()) {
                    let mut result = body.clone();
                    result["username"] = serde_json::json!(username.to_lowercase());
                    result
                } else {
                    body.clone()
                }
            }
            _ => body.clone(),
        }
    }
}

/// Build the default transformer with rules for the tipjar API.
pub fn default_transformer() -> RequestTransformer {
    let mut t = RequestTransformer::new();

    // Normalise tip amounts on both v1 and v2 write paths.
    t.add_rule(TransformationRule {
        path_prefix: "/api/v1/tips".to_string(),
        add_headers: HashMap::new(),
        remove_headers: vec![],
        body_transform: Some("normalize_amount".to_string()),
    });
    t.add_rule(TransformationRule {
        path_prefix: "/api/v2/tips".to_string(),
        add_headers: HashMap::new(),
        remove_headers: vec![],
        body_transform: Some("normalize_amount".to_string()),
    });

    // Lowercase usernames on creator creation.
    t.add_rule(TransformationRule {
        path_prefix: "/api/v1/creators".to_string(),
        add_headers: HashMap::new(),
        remove_headers: vec![],
        body_transform: Some("lowercase_username".to_string()),
    });
    t.add_rule(TransformationRule {
        path_prefix: "/api/v2/creators".to_string(),
        add_headers: HashMap::new(),
        remove_headers: vec![],
        body_transform: Some("lowercase_username".to_string()),
    });

    t
}

// ── Axum middleware ───────────────────────────────────────────────────────────

/// Axum middleware that applies request transformations.
///
/// For POST/PUT/PATCH requests with a JSON body, the body is buffered,
/// transformed, and re-injected.  Header additions/removals are applied to
/// all matching requests regardless of method.
///
/// Non-matching requests pass through unchanged.
pub async fn transform_request(req: Request, next: Next) -> Response {
    let transformer = default_transformer();
    let method = req.method().clone();
    let path = req.uri().path().to_owned();

    let Some(rule) = transformer.find_rule(&path) else {
        return next.run(req).await;
    };

    let (mut parts, body) = req.into_parts();

    // ── Header mutations ──────────────────────────────────────────────────────
    for name in &rule.remove_headers {
        if let Ok(header_name) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            parts.headers.remove(&header_name);
        }
    }
    for (name, value) in &rule.add_headers {
        if let (Ok(hn), Ok(hv)) = (
            axum::http::HeaderName::from_bytes(name.as_bytes()),
            axum::http::HeaderValue::from_str(value),
        ) {
            parts.headers.insert(hn, hv);
        }
    }

    // ── Body transform (JSON POST/PUT/PATCH only) ─────────────────────────────
    let body = if rule.body_transform.is_some()
        && matches!(
            method,
            axum::http::Method::POST | axum::http::Method::PUT | axum::http::Method::PATCH
        ) {
        match to_bytes(body, 1_048_576).await {
            Ok(bytes) => {
                if let Ok(mut json) = serde_json::from_slice::<Value>(&bytes) {
                    if let Some(transform) = &rule.body_transform {
                        json = RequestTransformer::apply_body_transform(&json, transform);
                    }
                    match serde_json::to_vec(&json) {
                        Ok(new_bytes) => {
                            // Update Content-Length to match the (possibly changed) body.
                            if let Ok(len_val) =
                                axum::http::HeaderValue::from_str(&new_bytes.len().to_string())
                            {
                                parts
                                    .headers
                                    .insert(axum::http::header::CONTENT_LENGTH, len_val);
                            }
                            Body::from(new_bytes)
                        }
                        Err(_) => Body::from(bytes),
                    }
                } else {
                    Body::from(bytes)
                }
            }
            Err(_) => Body::empty(),
        }
    } else {
        body
    };

    next.run(Request::from_parts(parts, body)).await
}
