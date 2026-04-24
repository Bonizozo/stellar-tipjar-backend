use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Generates an HMAC-SHA256 signature over `"{timestamp}.{payload}"`.
pub fn generate_signature(secret: &str, payload: &str, timestamp: i64) -> String {
    let message = format!("{}.{}", timestamp, payload);
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time comparison to prevent timing attacks.
pub fn verify_signature(secret: &str, payload: &str, timestamp: i64, signature: &str) -> bool {
    let expected = generate_signature(secret, payload, timestamp);
    constant_time_eq(expected.as_bytes(), signature.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let sig = generate_signature("secret", r#"{"foo":"bar"}"#, 1_700_000_000);
        assert!(verify_signature("secret", r#"{"foo":"bar"}"#, 1_700_000_000, &sig));
    }

    #[test]
    fn wrong_secret_fails() {
        let sig = generate_signature("secret", "body", 1_000);
        assert!(!verify_signature("other", "body", 1_000, &sig));
    }

    #[test]
    fn wrong_timestamp_fails() {
        let sig = generate_signature("secret", "body", 1_000);
        assert!(!verify_signature("secret", "body", 1_001, &sig));
    }
}
