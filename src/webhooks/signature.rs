use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Generates an HMAC-SHA256 signature for a webhook payload.
/// This allows the receiver to verify that the request came from Nova Launch.
pub fn generate_signature(secret: &str, payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_generation() {
        let secret = "test_secret";
        let payload = r#"{"event":"test"}"#;
        let sig = generate_signature(secret, payload);
        assert!(!sig.is_empty());
        
        // Re-generating with same input should yield same signature
        assert_eq!(sig, generate_signature(secret, payload));
    }
}
