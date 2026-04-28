//! Anonymization algorithms: masking, pseudonymization, generalization.

use super::detector::{PiiDetector, PiiField};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub struct Anonymizer {
    detector: PiiDetector,
}

impl Anonymizer {
    pub fn new() -> Self {
        Self {
            detector: PiiDetector::new(),
        }
    }

    /// Mask a string value, replacing PII with type-appropriate placeholders.
    pub fn mask_text(&self, text: &str) -> String {
        let detections = self.detector.detect(text);
        if detections.is_empty() {
            return text.to_string();
        }

        let mut result = text.to_string();
        // Process in reverse order to preserve offsets
        for det in detections.iter().rev() {
            let replacement = match det.field {
                PiiField::Email => "[EMAIL]".to_string(),
                PiiField::Phone => "[PHONE]".to_string(),
                PiiField::IpAddress => "[IP]".to_string(),
                PiiField::WalletAddress => format!("G...{}", &det.value[det.value.len().saturating_sub(4)..]),
                PiiField::Name => "[NAME]".to_string(),
            };
            result.replace_range(det.start..det.end, &replacement);
        }
        result
    }

    /// Pseudonymize a value using a one-way hash (HMAC-like with a salt).
    pub fn pseudonymize(&self, value: &str, salt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(salt.as_bytes());
        hasher.update(b":");
        hasher.update(value.as_bytes());
        let hash = hasher.finalize();
        hex::encode(&hash[..8]) // 16-char hex prefix
    }

    /// Anonymize a JSON object by masking known PII fields.
    pub fn anonymize_json(&self, value: &Value, pii_fields: &[&str]) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = Map::new();
                for (k, v) in map {
                    if pii_fields.contains(&k.as_str()) {
                        new_map.insert(k.clone(), Value::String(self.mask_field(v)));
                    } else {
                        new_map.insert(k.clone(), self.anonymize_json(v, pii_fields));
                    }
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.anonymize_json(v, pii_fields)).collect())
            }
            Value::String(s) => {
                if self.detector.contains_pii(s) {
                    Value::String(self.mask_text(s))
                } else {
                    value.clone()
                }
            }
            _ => value.clone(),
        }
    }

    fn mask_field(&self, value: &Value) -> String {
        match value {
            Value::String(s) => self.mask_text(s),
            _ => "[REDACTED]".to_string(),
        }
    }

    /// Generalize an IP address to /24 subnet (e.g. 1.2.3.4 -> 1.2.3.0/24)
    pub fn generalize_ip(ip: &str) -> String {
        let parts: Vec<&str> = ip.split('.').collect();
        if parts.len() == 4 {
            format!("{}.{}.{}.0/24", parts[0], parts[1], parts[2])
        } else {
            "[IP]".to_string()
        }
    }
}

impl Default for Anonymizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard PII field names used across the application
pub const STANDARD_PII_FIELDS: &[&str] = &[
    "email",
    "phone",
    "ip_address",
    "wallet_address",
    "recipient_email",
    "sender_email",
];
