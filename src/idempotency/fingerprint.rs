use sha2::{Digest, Sha256};

/// Fingerprint of a mutating request: `sha256(method | path | body)`.
///
/// Used to detect a client reusing the same `Idempotency-Key` for a
/// materially different request, which must be rejected with 422 rather
/// than silently replaying (or worse, re-executing) the wrong operation.
pub fn compute_fingerprint(method: &str, path: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(method.as_bytes());
    hasher.update(b"\x00");
    hasher.update(path.as_bytes());
    hasher.update(b"\x00");
    hasher.update(body);
    hex::encode(hasher.finalize())
}

/// Scope hash used as the Redis key / Postgres unique key for a given
/// `(principal, route, idempotency-key)` triple.
pub fn compute_scope_hash(principal: &str, route: &str, idempotency_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(principal.as_bytes());
    hasher.update(b"\x00");
    hasher.update(route.as_bytes());
    hasher.update(b"\x00");
    hasher.update(idempotency_key.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_deterministic() {
        let a = compute_fingerprint("POST", "/tips", b"{\"amount\":100}");
        let b = compute_fingerprint("POST", "/tips", b"{\"amount\":100}");
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_changes_with_body() {
        let a = compute_fingerprint("POST", "/tips", b"{\"amount\":100}");
        let b = compute_fingerprint("POST", "/tips", b"{\"amount\":200}");
        assert_ne!(a, b);
    }

    #[test]
    fn scope_hash_is_stable_and_distinguishes_keys() {
        let a = compute_scope_hash(
            "user:1",
            "POST /tips",
            "11111111-1111-1111-1111-111111111111",
        );
        let b = compute_scope_hash(
            "user:1",
            "POST /tips",
            "22222222-2222-2222-2222-222222222222",
        );
        let c = compute_scope_hash(
            "user:1",
            "POST /tips",
            "11111111-1111-1111-1111-111111111111",
        );
        assert_ne!(a, b);
        assert_eq!(a, c);
    }
}
