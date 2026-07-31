//! Versioned, timestamped webhook signature scheme for Stellar TipJar.
//!
//! # Format
//!
//! Every delivery sets the header:
//! ```text
//! X-TipJar-Signature: t=<unix_ts>,v1=<hmac_hex>[,v1=<hmac_hex2>]
//! ```
//!
//! The signed message is `"{unix_ts}.{raw_body}"` — binding the timestamp to
//! the payload so that replayed deliveries outside the tolerance window can be
//! rejected by receivers.
//!
//! During secret rotation two `v1=` values are emitted (one per active secret).
//! Receivers must accept a delivery if **any** `v1` value matches.
//!
//! # Tolerance window
//!
//! The default replay-protection window is **5 minutes** (`SIGNATURE_TOLERANCE_SECS`).
//! Receivers should reject any delivery whose `t=` timestamp is more than
//! this many seconds in the past (or future).
//!
//! # Receiver verification (constant-time)
//!
//! See `docs/WEBHOOK_VERIFICATION.md` for language-specific examples.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Default replay-protection tolerance window in seconds (5 minutes).
pub const SIGNATURE_TOLERANCE_SECS: u64 = 300;

// ── Signing ───────────────────────────────────────────────────────────────────

/// Compute one HMAC-SHA256 `v1` value.
///
/// The signed message is `"{timestamp}.{raw_body}"`.
fn compute_v1(secret: &str, timestamp: u64, raw_body: &str) -> String {
    let signed_payload = format!("{}.{}", timestamp, raw_body);
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(signed_payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Build the full `X-TipJar-Signature` header value for one or more secrets.
///
/// With a single secret the output is:
/// ```text
/// t=1715000000,v1=abcd1234…
/// ```
///
/// With two secrets (during rotation):
/// ```text
/// t=1715000000,v1=abcd1234…,v1=efgh5678…
/// ```
pub fn build_signature_header(secrets: &[&str], raw_body: &str) -> String {
    build_signature_header_at(secrets, raw_body, current_unix_ts())
}

/// Same as `build_signature_header` but accepts an explicit timestamp — used
/// in tests to control the clock.
pub fn build_signature_header_at(secrets: &[&str], raw_body: &str, timestamp: u64) -> String {
    let mut parts = vec![format!("t={}", timestamp)];
    for secret in secrets {
        parts.push(format!("v1={}", compute_v1(secret, timestamp, raw_body)));
    }
    parts.join(",")
}

// ── Verification ──────────────────────────────────────────────────────────────

/// Parse the `X-TipJar-Signature` header into its components.
///
/// Returns `None` if the header is malformed (missing `t=`).
pub struct ParsedSignature {
    /// Unix timestamp from the `t=` component.
    pub timestamp: u64,
    /// All `v1=` hex values present in the header.
    pub v1_values: Vec<String>,
}

pub fn parse_signature_header(header: &str) -> Option<ParsedSignature> {
    let mut timestamp = None;
    let mut v1_values = Vec::new();

    for part in header.split(',') {
        let part = part.trim();
        if let Some(ts_str) = part.strip_prefix("t=") {
            timestamp = ts_str.parse::<u64>().ok();
        } else if let Some(sig) = part.strip_prefix("v1=") {
            v1_values.push(sig.to_string());
        }
    }

    Some(ParsedSignature {
        timestamp: timestamp?,
        v1_values,
    })
}

/// Verify an inbound webhook delivery.
///
/// Returns `Ok(())` when:
/// 1. The header is well-formed.
/// 2. The timestamp is within `tolerance_secs` of now.
/// 3. At least one `v1=` value matches `HMAC(secret, "{ts}.{body}")`.
///
/// All HMAC comparisons are **constant-time** to prevent timing attacks.
///
/// # Arguments
/// * `signature_header` — full value of `X-TipJar-Signature`
/// * `raw_body`         — raw (unmodified) request body bytes as UTF-8 string
/// * `secret`           — the webhook secret configured for this endpoint
/// * `tolerance_secs`   — maximum age of a valid delivery in seconds
pub fn verify_signature(
    signature_header: &str,
    raw_body: &str,
    secret: &str,
    tolerance_secs: u64,
) -> Result<(), SignatureError> {
    verify_signature_at(
        signature_header,
        raw_body,
        secret,
        tolerance_secs,
        current_unix_ts(),
    )
}

/// Same as `verify_signature` but with an explicit current time — used in tests.
pub fn verify_signature_at(
    signature_header: &str,
    raw_body: &str,
    secret: &str,
    tolerance_secs: u64,
    now: u64,
) -> Result<(), SignatureError> {
    let parsed = parse_signature_header(signature_header).ok_or(SignatureError::MalformedHeader)?;

    // Tolerance check — guard against replay AND clock skew.
    let age = now.saturating_sub(parsed.timestamp);
    let skew = parsed.timestamp.saturating_sub(now);
    if age > tolerance_secs || skew > tolerance_secs {
        return Err(SignatureError::TimestampOutOfWindow {
            timestamp: parsed.timestamp,
            now,
            tolerance_secs,
        });
    }

    if parsed.v1_values.is_empty() {
        return Err(SignatureError::NoSignatureValues);
    }

    // Compute the expected HMAC.
    let expected = compute_v1(secret, parsed.timestamp, raw_body);
    let expected_bytes = expected.as_bytes();

    // Accept if any v1= value matches — constant-time compare each.
    let matched = parsed
        .v1_values
        .iter()
        .any(|v| constant_time_eq(v.as_bytes(), expected_bytes));

    if matched {
        Ok(())
    } else {
        Err(SignatureError::SignatureMismatch)
    }
}

/// Errors returned by `verify_signature`.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SignatureError {
    #[error("X-TipJar-Signature header is malformed or missing required components")]
    MalformedHeader,
    #[error("no v1= values found in signature header")]
    NoSignatureValues,
    #[error(
        "delivery timestamp {timestamp} is outside the {tolerance_secs}s tolerance window (now={now})"
    )]
    TimestampOutOfWindow {
        timestamp: u64,
        now: u64,
        tolerance_secs: u64,
    },
    #[error("HMAC signature does not match any known secret")]
    SignatureMismatch,
}

// ── Legacy compatibility ──────────────────────────────────────────────────────

/// **Deprecated.** Bare HMAC-SHA256 of the payload with no timestamp binding.
///
/// Retained only for the migration period. All new code must use
/// `build_signature_header` / `verify_signature` instead.
#[deprecated(
    since = "0.2.0",
    note = "Use `build_signature_header` + `verify_signature` for timestamped, versioned signatures"
)]
pub fn generate_signature(secret: &str, payload: &str) -> String {
    compute_v1(secret, 0, payload)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return the current Unix timestamp in seconds.
pub fn current_unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Constant-time byte-slice equality — prevents timing side-channel attacks.
///
/// Returns `true` iff `a` and `b` have the same length and identical contents,
/// evaluated in constant time regardless of where the first difference occurs.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_test_secret_abc123";
    const BODY: &str = r#"{"event":"tip.created","amount":"10.0"}"#;
    const NOW: u64 = 1_715_000_000;

    // ── Build / parse round-trip ──────────────────────────────────────────

    #[test]
    fn header_format_single_secret() {
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        assert!(header.starts_with("t=1715000000,v1="), "header={header}");
        // No duplicate v1= sections.
        assert_eq!(header.matches("v1=").count(), 1);
    }

    #[test]
    fn header_format_dual_secret_rotation() {
        let secret2 = "whsec_new_secret_xyz789";
        let header = build_signature_header_at(&[SECRET, secret2], BODY, NOW);
        assert!(header.starts_with("t=1715000000,v1="), "header={header}");
        // Two v1= values during rotation.
        assert_eq!(header.matches("v1=").count(), 2);
        // Both secrets must produce different signatures.
        let parts: Vec<&str> = header.split(',').collect();
        let sig1 = parts[1];
        let sig2 = parts[2];
        assert_ne!(
            sig1, sig2,
            "different secrets must produce different v1 values"
        );
    }

    #[test]
    fn parse_extracts_timestamp_and_v1() {
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        let parsed = parse_signature_header(&header).unwrap();
        assert_eq!(parsed.timestamp, NOW);
        assert_eq!(parsed.v1_values.len(), 1);
    }

    #[test]
    fn parse_malformed_returns_none() {
        assert!(parse_signature_header("no-equals-sign").is_none());
        assert!(parse_signature_header("").is_none());
        assert!(parse_signature_header("v1=abc").is_none()); // missing t=
    }

    // ── Verification: valid cases ─────────────────────────────────────────

    #[test]
    fn verify_valid_signature() {
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();
    }

    #[test]
    fn verify_accepts_first_of_dual_secrets() {
        let secret2 = "whsec_new_secret_xyz789";
        let header = build_signature_header_at(&[SECRET, secret2], BODY, NOW);
        // Old secret still works during rotation.
        verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();
        // New secret also works.
        verify_signature_at(&header, BODY, secret2, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();
    }

    // ── Tolerance boundary tests (exactly ±1 s around the window) ─────────

    #[test]
    fn verify_at_exact_tolerance_boundary_is_valid() {
        // Delivery is exactly tolerance_secs old — still within window.
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        let exactly_at_boundary = NOW + SIGNATURE_TOLERANCE_SECS;
        verify_signature_at(
            &header,
            BODY,
            SECRET,
            SIGNATURE_TOLERANCE_SECS,
            exactly_at_boundary,
        )
        .unwrap();
    }

    #[test]
    fn verify_one_second_past_tolerance_is_rejected() {
        // Delivery is tolerance_secs + 1 old — outside window.
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        let one_past = NOW + SIGNATURE_TOLERANCE_SECS + 1;
        let err = verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, one_past)
            .unwrap_err();
        assert!(
            matches!(err, SignatureError::TimestampOutOfWindow { .. }),
            "expected TimestampOutOfWindow, got {err:?}"
        );
    }

    #[test]
    fn verify_one_second_before_tolerance_is_valid() {
        // Delivery is tolerance_secs - 1 old — comfortably inside window.
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        let one_before = NOW + SIGNATURE_TOLERANCE_SECS - 1;
        verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, one_before).unwrap();
    }

    #[test]
    fn verify_future_timestamp_beyond_tolerance_is_rejected() {
        // Delivery claims a timestamp far in the future — reject clock skew attacks.
        let header = build_signature_header_at(&[SECRET], BODY, NOW + SIGNATURE_TOLERANCE_SECS + 1);
        let err =
            verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW).unwrap_err();
        assert!(matches!(err, SignatureError::TimestampOutOfWindow { .. }));
    }

    // ── Verification: invalid cases ───────────────────────────────────────

    #[test]
    fn verify_wrong_secret_fails() {
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        let err = verify_signature_at(&header, BODY, "wrong_secret", SIGNATURE_TOLERANCE_SECS, NOW)
            .unwrap_err();
        assert_eq!(err, SignatureError::SignatureMismatch);
    }

    #[test]
    fn verify_tampered_body_fails() {
        let header = build_signature_header_at(&[SECRET], BODY, NOW);
        let err = verify_signature_at(
            &header,
            r#"{"event":"tip.created","amount":"99999.0"}"#,
            SECRET,
            SIGNATURE_TOLERANCE_SECS,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::SignatureMismatch);
    }

    #[test]
    fn verify_malformed_header_fails() {
        let err = verify_signature_at("garbage", BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW)
            .unwrap_err();
        assert_eq!(err, SignatureError::MalformedHeader);
    }

    #[test]
    fn verify_header_without_v1_fails() {
        let err = verify_signature_at("t=1715000000", BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW)
            .unwrap_err();
        assert_eq!(err, SignatureError::NoSignatureValues);
    }

    // ── Dual-secret rotation overlap ──────────────────────────────────────

    #[test]
    fn rotation_overlap_no_missed_verifications() {
        let old_secret = "old_secret_being_rotated";
        let new_secret = "new_secret_now_primary";

        // Phase 1: before rotation — server signs with old secret only.
        let header_before = build_signature_header_at(&[old_secret], BODY, NOW);
        // Receiver using old secret: passes.
        verify_signature_at(
            &header_before,
            BODY,
            old_secret,
            SIGNATURE_TOLERANCE_SECS,
            NOW,
        )
        .unwrap();

        // Phase 2: during overlap — server signs with BOTH secrets.
        let header_overlap = build_signature_header_at(&[old_secret, new_secret], BODY, NOW);
        // Receiver still using old secret: passes (v1 for old secret is present).
        verify_signature_at(
            &header_overlap,
            BODY,
            old_secret,
            SIGNATURE_TOLERANCE_SECS,
            NOW,
        )
        .unwrap();
        // Receiver already updated to new secret: also passes.
        verify_signature_at(
            &header_overlap,
            BODY,
            new_secret,
            SIGNATURE_TOLERANCE_SECS,
            NOW,
        )
        .unwrap();

        // Phase 3: after rotation — server signs with new secret only.
        let header_after = build_signature_header_at(&[new_secret], BODY, NOW);
        // Receiver using new secret: passes.
        verify_signature_at(
            &header_after,
            BODY,
            new_secret,
            SIGNATURE_TOLERANCE_SECS,
            NOW,
        )
        .unwrap();
        // Receiver still on old secret: fails — rotation is complete.
        let err = verify_signature_at(
            &header_after,
            BODY,
            old_secret,
            SIGNATURE_TOLERANCE_SECS,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::SignatureMismatch);
    }

    // ── Constant-time equality ────────────────────────────────────────────

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }
}
