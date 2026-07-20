use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Generates an HMAC-SHA256 signature for a webhook payload.
/// The receiver can verify the request origin with this signature.
pub fn generate_signature(secret: &str, payload: &str) -> String {
    // SAFETY: `Hmac::new_from_slice` accepts any non-empty key size for
    // HMAC-SHA256.  The only error case (InvalidLength) is unreachable here
    // because SHA-256 HMAC accepts keys of any length.
    // Invariant: `new_from_slice` with SHA-256 never returns Err.
    #[allow(clippy::expect_used)]
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length — unreachable");
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

        // Same input → same signature (deterministic).
        assert_eq!(sig, generate_signature(secret, payload));
    }

    #[test]
    fn different_secrets_produce_different_signatures() {
        let payload = "data";
        let s1 = generate_signature("secret1", payload);
        let s2 = generate_signature("secret2", payload);
        assert_ne!(s1, s2);
    }
}
