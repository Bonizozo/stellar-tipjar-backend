//! PII detection using regex patterns.

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PiiField {
    Email,
    Phone,
    IpAddress,
    WalletAddress,
    Name,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiDetection {
    pub field: PiiField,
    pub value: String,
    pub start: usize,
    pub end: usize,
}

pub struct PiiDetector {
    email_re: Regex,
    phone_re: Regex,
    ip_re: Regex,
    wallet_re: Regex,
}

impl PiiDetector {
    pub fn new() -> Self {
        Self {
            email_re: Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap(),
            phone_re: Regex::new(r"\+?[0-9]{7,15}").unwrap(),
            ip_re: Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
            // Stellar public key: G + 55 base32 chars
            wallet_re: Regex::new(r"\bG[A-Z2-7]{55}\b").unwrap(),
        }
    }

    /// Detect all PII occurrences in a text string.
    pub fn detect(&self, text: &str) -> Vec<PiiDetection> {
        let mut detections = Vec::new();

        for m in self.email_re.find_iter(text) {
            detections.push(PiiDetection {
                field: PiiField::Email,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in self.phone_re.find_iter(text) {
            detections.push(PiiDetection {
                field: PiiField::Phone,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in self.ip_re.find_iter(text) {
            detections.push(PiiDetection {
                field: PiiField::IpAddress,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in self.wallet_re.find_iter(text) {
            detections.push(PiiDetection {
                field: PiiField::WalletAddress,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }

        // Sort by position for deterministic output
        detections.sort_by_key(|d| d.start);
        detections
    }

    /// Returns true if the text contains any PII.
    pub fn contains_pii(&self, text: &str) -> bool {
        self.email_re.is_match(text)
            || self.ip_re.is_match(text)
            || self.wallet_re.is_match(text)
    }
}

impl Default for PiiDetector {
    fn default() -> Self {
        Self::new()
    }
}
