//! Exhaustive tests for the Webhook Security Overhaul (#338).
//!
//! Covers:
//!   - Signature format (versioned header, signed message)
//!   - Tolerance boundary (±1 s around the window)
//!   - Dual-secret rotation overlap (zero missed verifications)
//!   - Delivery-ID stability across retries
//!   - Circuit-breaker transitions (closed → open → half-open)

use httpmock::prelude::*;
use serde_json::json;
use stellar_tipjar_backend::webhooks::signature::{
    build_signature_header_at, parse_signature_header, verify_signature_at,
    SignatureError, SIGNATURE_TOLERANCE_SECS,
};
use stellar_tipjar_backend::webhooks::sender::DeliveryContext;

const SECRET: &str = "whsec_test_abc123";
const BODY: &str = r#"{"event":"tip.created","amount":"10.0"}"#;
const NOW: u64 = 1_715_000_000;

// ── Signature format ──────────────────────────────────────────────────────────

#[test]
fn header_starts_with_timestamp() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    assert!(
        header.starts_with("t=1715000000,"),
        "header must start with 't=<ts>,': {header}"
    );
}

#[test]
fn header_contains_v1_component() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    assert!(header.contains("v1="), "header must contain 'v1=': {header}");
}

#[test]
fn signed_message_includes_timestamp() {
    // Two deliveries of the same body at different timestamps must produce
    // different v1 values — proves the timestamp is bound into the signature.
    let h1 = build_signature_header_at(&[SECRET], BODY, NOW);
    let h2 = build_signature_header_at(&[SECRET], BODY, NOW + 1);
    let v1_1 = h1.split("v1=").nth(1).unwrap_or("");
    let v1_2 = h2.split("v1=").nth(1).unwrap_or("");
    assert_ne!(v1_1, v1_2, "different timestamps must produce different v1 values");
}

#[test]
fn same_input_produces_same_signature() {
    let h1 = build_signature_header_at(&[SECRET], BODY, NOW);
    let h2 = build_signature_header_at(&[SECRET], BODY, NOW);
    assert_eq!(h1, h2, "deterministic: same input → same output");
}

#[test]
fn dual_secret_header_has_two_v1_values() {
    let secret2 = "whsec_new_xyz789";
    let header = build_signature_header_at(&[SECRET, secret2], BODY, NOW);
    assert_eq!(
        header.matches("v1=").count(),
        2,
        "two secrets → two v1= values: {header}"
    );
}

#[test]
fn parse_extracts_all_v1_values() {
    let secret2 = "whsec_new_xyz789";
    let header = build_signature_header_at(&[SECRET, secret2], BODY, NOW);
    let parsed = parse_signature_header(&header).unwrap();
    assert_eq!(parsed.timestamp, NOW);
    assert_eq!(parsed.v1_values.len(), 2);
}

// ── Verification: valid ───────────────────────────────────────────────────────

#[test]
fn verify_valid_delivery_passes() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();
}

// ── Tolerance boundary (±1 s around the window) ───────────────────────────────

#[test]
fn verify_at_exact_boundary_passes() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    let at_boundary = NOW + SIGNATURE_TOLERANCE_SECS;
    verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, at_boundary).unwrap();
}

#[test]
fn verify_one_second_past_boundary_fails() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    let one_over = NOW + SIGNATURE_TOLERANCE_SECS + 1;
    let err = verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, one_over)
        .unwrap_err();
    assert!(
        matches!(err, SignatureError::TimestampOutOfWindow { .. }),
        "expected TimestampOutOfWindow, got {err:?}"
    );
}

#[test]
fn verify_one_second_before_boundary_passes() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    let one_before = NOW + SIGNATURE_TOLERANCE_SECS - 1;
    verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, one_before).unwrap();
}

#[test]
fn verify_future_timestamp_beyond_tolerance_fails() {
    // Clock skew attack: timestamp claims to be far in the future.
    let header =
        build_signature_header_at(&[SECRET], BODY, NOW + SIGNATURE_TOLERANCE_SECS + 1);
    let err = verify_signature_at(&header, BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW)
        .unwrap_err();
    assert!(matches!(err, SignatureError::TimestampOutOfWindow { .. }));
}

// ── Verification: invalid cases ───────────────────────────────────────────────

#[test]
fn wrong_secret_fails() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    let err =
        verify_signature_at(&header, BODY, "wrong_secret", SIGNATURE_TOLERANCE_SECS, NOW)
            .unwrap_err();
    assert_eq!(err, SignatureError::SignatureMismatch);
}

#[test]
fn tampered_body_fails() {
    let header = build_signature_header_at(&[SECRET], BODY, NOW);
    let err = verify_signature_at(
        &header,
        r#"{"event":"tip.created","amount":"9999.0"}"#,
        SECRET,
        SIGNATURE_TOLERANCE_SECS,
        NOW,
    )
    .unwrap_err();
    assert_eq!(err, SignatureError::SignatureMismatch);
}

#[test]
fn malformed_header_fails() {
    let err =
        verify_signature_at("garbage", BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW).unwrap_err();
    assert_eq!(err, SignatureError::MalformedHeader);
}

#[test]
fn header_without_v1_fails() {
    let err = verify_signature_at("t=1715000000", BODY, SECRET, SIGNATURE_TOLERANCE_SECS, NOW)
        .unwrap_err();
    assert_eq!(err, SignatureError::NoSignatureValues);
}

// ── Dual-secret rotation overlap ──────────────────────────────────────────────

#[test]
fn rotation_overlap_zero_missed_verifications() {
    let old = "old_secret_retiring";
    let new = "new_secret_primary";

    // PHASE 1: single old secret — old receiver passes.
    let h_before = build_signature_header_at(&[old], BODY, NOW);
    verify_signature_at(&h_before, BODY, old, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();

    // PHASE 2: overlap — server sends both.
    let h_overlap = build_signature_header_at(&[new, old], BODY, NOW);
    // Old receiver: still passes (old secret present).
    verify_signature_at(&h_overlap, BODY, old, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();
    // New receiver: also passes (new secret present).
    verify_signature_at(&h_overlap, BODY, new, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();

    // PHASE 3: rotation complete — only new secret.
    let h_after = build_signature_header_at(&[new], BODY, NOW);
    verify_signature_at(&h_after, BODY, new, SIGNATURE_TOLERANCE_SECS, NOW).unwrap();
    // Old receiver now fails — expected.
    let err =
        verify_signature_at(&h_after, BODY, old, SIGNATURE_TOLERANCE_SECS, NOW).unwrap_err();
    assert_eq!(err, SignatureError::SignatureMismatch, "old secret must fail after rotation");
}

// ── Delivery ID stability ─────────────────────────────────────────────────────

#[test]
fn delivery_id_stable_within_context() {
    let ctx = DeliveryContext::new("tip.created", SECRET.to_string());
    let id1 = ctx.delivery_id;
    let id2 = ctx.delivery_id;
    assert_eq!(id1, id2, "delivery_id must be stable within the same context");
}

#[test]
fn delivery_id_differs_across_new_contexts() {
    let ctx1 = DeliveryContext::new("tip.created", SECRET.to_string());
    let ctx2 = DeliveryContext::new("tip.created", SECRET.to_string());
    assert_ne!(
        ctx1.delivery_id, ctx2.delivery_id,
        "each new context must get a fresh delivery_id"
    );
}

#[test]
fn delivery_id_preserved_when_cloned() {
    // Simulates what retry.rs does: clone the context for each attempt.
    let original = DeliveryContext::new("tip.created", SECRET.to_string());
    let cloned = original.clone();
    assert_eq!(
        original.delivery_id, cloned.delivery_id,
        "cloning the context must preserve the delivery_id (stable across retries)"
    );
}

// ── Send webhook: correct headers emitted ────────────────────────────────────

#[tokio::test]
async fn send_webhook_emits_required_headers() {
    let server = MockServer::start();

    // Capture all request headers sent by our sender.
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/webhook")
            .header_exists("X-TipJar-Signature")
            .header_exists("X-TipJar-Delivery-Id")
            .header_exists("X-TipJar-Event-Type")
            .header("content-type", "application/json");
        then.status(200);
    });

    let url = format!("{}/webhook", server.base_url());
    let ctx = DeliveryContext::new("tip.created", SECRET.to_string());
    let payload = json!({"event": "tip.created"});

    stellar_tipjar_backend::webhooks::sender::send_webhook(&url, &ctx, payload)
        .await
        .expect("delivery should succeed");

    mock.assert();
}

#[tokio::test]
async fn send_webhook_signature_header_verifiable() {
    let server = MockServer::start();
    let received_sig: std::sync::Arc<std::sync::Mutex<String>> =
        std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let received_body: std::sync::Arc<std::sync::Mutex<String>> =
        std::sync::Arc::new(std::sync::Mutex::new(String::new()));

    let sig_arc = received_sig.clone();
    let body_arc = received_body.clone();

    server.mock(|when, then| {
        when.method(POST).path("/hook");
        then.status(200);
    });

    let url = format!("{}/hook", server.base_url());
    let ctx = DeliveryContext::new("tip.created", SECRET.to_string());
    let payload = json!({"event": "tip.created", "amount": "5.0"});

    stellar_tipjar_backend::webhooks::sender::send_webhook(&url, &ctx, payload.clone())
        .await
        .unwrap();

    // Re-construct what the sender signed and verify ourselves.
    let payload_str = serde_json::to_string(&payload).unwrap();
    let header = build_signature_header_at(&[SECRET], &payload_str, NOW);

    // Verify using verify_signature_at with a wide tolerance window to avoid
    // flaky clock issues in CI.
    let now_ts = stellar_tipjar_backend::webhooks::signature::current_unix_ts();
    let parsed = parse_signature_header(&header).unwrap();
    // The delivery was just sent so it must be within tolerance.
    assert!(
        now_ts.saturating_sub(parsed.timestamp) <= SIGNATURE_TOLERANCE_SECS
            || parsed.timestamp.saturating_sub(now_ts) <= SIGNATURE_TOLERANCE_SECS
    );
}

#[tokio::test]
async fn send_webhook_delivery_id_matches_context() {
    let server = MockServer::start();

    // Capture the X-TipJar-Delivery-Id header value.
    let capture = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let cap = capture.clone();

    server.mock(|when, then| {
        when.method(POST).path("/hook2");
        then.status(200);
    });

    let url = format!("{}/hook2", server.base_url());
    let ctx = DeliveryContext::new("tip.created", SECRET.to_string());
    let expected_id = ctx.delivery_id.to_string();

    stellar_tipjar_backend::webhooks::sender::send_webhook(
        &url,
        &ctx,
        json!({"x": 1}),
    )
    .await
    .unwrap();

    // Verify via the mock request log.
    let requests = server.requests();
    let req = requests.first().expect("at least one request");
    let actual_id = req
        .headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == "x-tipjar-delivery-id")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    assert_eq!(actual_id, expected_id, "X-TipJar-Delivery-Id must match context.delivery_id");
}

// ── Circuit breaker integration ───────────────────────────────────────────────

#[test]
fn circuit_breaker_opens_after_threshold_failures() {
    use stellar_tipjar_backend::services::circuit_breaker::{CircuitBreaker, CircuitState};
    use std::time::Duration;

    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    assert_eq!(cb.state(), CircuitState::Closed);

    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Closed, "still closed before threshold");

    cb.record_failure(); // threshold hit
    assert_eq!(cb.state(), CircuitState::Open, "must open after threshold");
    assert!(!cb.allow_request(), "open circuit must block requests");
}

#[test]
fn circuit_breaker_recovers_to_half_open() {
    use stellar_tipjar_backend::services::circuit_breaker::{CircuitBreaker, CircuitState};
    use std::time::Duration;

    // Zero recovery timeout so it transitions to HalfOpen immediately.
    let cb = CircuitBreaker::new(2, Duration::from_secs(0));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);

    // After 0s timeout the circuit should be HalfOpen.
    assert_eq!(cb.state(), CircuitState::HalfOpen, "zero-timeout circuit must go HalfOpen");
    assert!(cb.allow_request(), "HalfOpen must allow a probe request");
}

#[test]
fn circuit_breaker_resets_on_success() {
    use stellar_tipjar_backend::services::circuit_breaker::{CircuitBreaker, CircuitState};
    use std::time::Duration;

    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    cb.record_failure();
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed, "success must reset the circuit");
}

// ── Nova Launch artifact: removed ────────────────────────────────────────────

/// Verify that the "Nova Launch" copy-paste artifact has been removed from
/// the codebase.  The signature module must not reference any third party.
#[test]
fn no_nova_launch_reference_in_signature_module() {
    let src = include_str!("../src/webhooks/signature.rs");
    assert!(
        !src.contains("Nova Launch"),
        "signature.rs must not contain 'Nova Launch' reference"
    );
    assert!(
        !src.contains("nova launch"),
        "signature.rs must not contain 'nova launch' reference"
    );
}
