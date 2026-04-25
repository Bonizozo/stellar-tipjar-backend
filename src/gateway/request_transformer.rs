use axum::http::Request;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RequestTransformer {
    transformations: HashMap<String, TransformationRule>,
}

#[derive(Debug, Clone)]
pub struct TransformationRule {
    pub path_pattern: String,
    pub add_headers: HashMap<String, String>,
    pub remove_headers: Vec<String>,
    pub body_transform: Option<String>,
}

impl RequestTransformer {
    pub fn new() -> Self {
        Self {
            transformations: HashMap::new(),
        }
    }

    pub fn add_rule(&mut self, path: String, rule: TransformationRule) {
        self.transformations.insert(path, rule);
    }

    pub fn transform_request(&self, path: &str, mut body: Value) -> Value {
        if let Some(rule) = self.transformations.get(path) {
            // Apply body transformations
            if let Some(transform) = &rule.body_transform {
                body = self.apply_transform(&body, transform);
            }
        }
        body
    }

    fn apply_transform(&self, body: &Value, transform: &str) -> Value {
        match transform {
            "normalize_amount" => {
                if let Some(amount) = body.get("amount").and_then(|v| v.as_str()) {
                    let mut result = body.clone();
                    result["amount"] = json!(amount.trim());
                    result
                } else {
                    body.clone()
                }
            }
            _ => body.clone(),
        }
    }
}

impl Default for RequestTransformer {
    fn default() -> Self {
        Self::new()
    }
}
