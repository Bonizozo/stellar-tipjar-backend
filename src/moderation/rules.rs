//! Rule-based moderation engine.
//!
//! Applies pattern-matching rules to detect spam, inappropriate content,
//! personal information leakage, and policy violations. All checks run
//! synchronously with no network calls.

use lazy_static::lazy_static;
use regex::Regex;

use super::{ContentType, Violation, ViolationType};

lazy_static! {
    // URLs embedded in user-supplied text are a common spam vector.
    static ref URL_PATTERN: Regex =
        Regex::new(r"https?://[^\s]{4,}").unwrap();

    // Classic get-rich-quick / crypto-scam phrasing.
    static ref SPAM_PHRASES: Regex = Regex::new(
        r"(?i)\b(buy\s+now|click\s+here|free\s+money|make\s+money\s+fast|crypto\s+giveaway|double\s+your|guaranteed\s+(profit|returns?)|act\s+now|limited\s+time\s+offer|earn\s+\$\d+)\b"
    ).unwrap();

    // E-mail addresses should not appear in public-facing fields.
    static ref EMAIL_PATTERN: Regex =
        Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap();

    // Basic phone number detection (international and local formats).
    static ref PHONE_PATTERN: Regex =
        Regex::new(r"(\+?\d[\d\s\-().]{7,}\d)").unwrap();

    // Hate speech slurs and targeted harassment patterns.
    // Kept intentionally vague to avoid false positives; the AI detector
    // handles nuanced cases.
    static ref HATE_SPEECH_PATTERN: Regex = Regex::new(
        r"(?i)\b(kys|kms|go\s+kill\s+yourself|i\s+will\s+(kill|harm|hurt)\s+you)\b"
    ).unwrap();
}

/// Usernames that are reserved and must not be registered.
const RESERVED_USERNAMES: &[&str] = &[
    "admin",
    "administrator",
    "root",
    "system",
    "support",
    "help",
    "api",
    "null",
    "undefined",
    "moderator",
    "mod",
    "staff",
    "official",
    "tipjar",
    "stellar",
];

/// Pattern-matching rule engine.
pub struct RulesEngine;

impl RulesEngine {
    pub fn new() -> Self {
        Self
    }

    /// True if `text` contains a run of 7 or more consecutive identical
    /// characters (e.g. "aaaaaaa", "!!!!!!!"). Checked with a manual scan
    /// rather than a regex backreference — the `regex` crate doesn't support
    /// backreferences, so `(.)\1{6,}` can never compile.
    fn has_repeated_chars(text: &str) -> bool {
        let mut chars = text.chars();
        let Some(mut prev) = chars.next() else {
            return false;
        };
        let mut run = 1;
        for c in chars {
            if c == prev {
                run += 1;
                if run >= 7 {
                    return true;
                }
            } else {
                prev = c;
                run = 1;
            }
        }
        false
    }

    /// Run all applicable rules for the given content type and return the list
    /// of detected violations.
    pub fn check(&self, content: &str, content_type: &ContentType) -> Vec<Violation> {
        let mut violations = Vec::new();

        match content_type {
            ContentType::Username => {
                self.check_reserved_username(content, &mut violations);
                self.check_spam_in_text(content, &mut violations);
            }
            ContentType::TipMessage | ContentType::CreatorBio => {
                self.check_spam_in_text(content, &mut violations);
                self.check_personal_information(content, &mut violations);
                self.check_hate_speech(content, &mut violations);
            }
        }

        violations
    }

    fn check_reserved_username(&self, username: &str, violations: &mut Vec<Violation>) {
        let lower = username.to_lowercase();

        if RESERVED_USERNAMES.contains(&lower.as_str()) {
            violations.push(Violation {
                violation_type: ViolationType::PolicyViolation,
                description: format!("'{}' is a reserved username", username),
                confidence: 1.0,
            });
        }
    }

    fn check_spam_in_text(&self, text: &str, violations: &mut Vec<Violation>) {
        let url_count = URL_PATTERN.find_iter(text).count();
        if url_count > 0 {
            violations.push(Violation {
                violation_type: ViolationType::Spam,
                description: format!("Content contains {} URL(s)", url_count),
                confidence: 0.75,
            });
        }

        if SPAM_PHRASES.is_match(text) {
            violations.push(Violation {
                violation_type: ViolationType::Spam,
                description: "Content matches known spam/scam phrasing".to_string(),
                confidence: 0.90,
            });
        }

        if Self::has_repeated_chars(text) {
            violations.push(Violation {
                violation_type: ViolationType::Spam,
                description: "Content contains excessive repeated characters".to_string(),
                confidence: 0.65,
            });
        }
    }

    fn check_personal_information(&self, text: &str, violations: &mut Vec<Violation>) {
        if EMAIL_PATTERN.is_match(text) {
            violations.push(Violation {
                violation_type: ViolationType::PersonalInformation,
                description: "Content may contain an email address".to_string(),
                confidence: 0.85,
            });
        }

        if PHONE_PATTERN.is_match(text) {
            violations.push(Violation {
                violation_type: ViolationType::PersonalInformation,
                description: "Content may contain a phone number".to_string(),
                confidence: 0.70,
            });
        }
    }

    fn check_hate_speech(&self, text: &str, violations: &mut Vec<Violation>) {
        if HATE_SPEECH_PATTERN.is_match(text) {
            violations.push(Violation {
                violation_type: ViolationType::HateSpeech,
                description: "Content matches hate speech / self-harm pattern".to_string(),
                confidence: 0.95,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> RulesEngine {
        RulesEngine::new()
    }

    #[test]
    fn reserved_username_flagged() {
        let v = engine().check("admin", &ContentType::Username);
        assert!(v
            .iter()
            .any(|x| x.violation_type == ViolationType::PolicyViolation));
    }

    #[test]
    fn normal_username_passes() {
        let v = engine().check("alice42", &ContentType::Username);
        assert!(v.is_empty());
    }

    #[test]
    fn spam_phrase_flagged() {
        let v = engine().check("FREE MONEY click here now", &ContentType::TipMessage);
        assert!(v.iter().any(|x| x.violation_type == ViolationType::Spam));
    }

    #[test]
    fn url_in_message_flagged() {
        let v = engine().check("Visit https://example.com/promo", &ContentType::TipMessage);
        assert!(v.iter().any(|x| x.violation_type == ViolationType::Spam));
    }

    #[test]
    fn email_in_message_flagged() {
        let v = engine().check("Contact me at foo@bar.com", &ContentType::TipMessage);
        assert!(v
            .iter()
            .any(|x| x.violation_type == ViolationType::PersonalInformation));
    }

    #[test]
    fn clean_message_passes() {
        let v = engine().check("Great work on the stream today!", &ContentType::TipMessage);
        assert!(v.is_empty());
    }
}
