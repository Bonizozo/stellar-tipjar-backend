//! Unit tests for the production-grade Stellar tip transaction pipeline.
//!
//! Covers every requirement from issue #530:
//!   - Dynamic fee: surge-fee path, floor/ceiling clamping
//!   - Memo validation: exact 28-byte boundary (28 ok, 29 rejected)
//!   - Destination account: funded vs unfunded paths
//!   - Spendable balance: normal, under-reserve, insufficient
//!   - Horizon error mapping: result codes, 429, 504 retry
//!   - tx_bad_seq, op_underfunded, op_no_destination result codes

use httpmock::prelude::*;
use serde_json::json;
use stellar_tipjar_backend::errors::{AppError, StellarError};
use stellar_tipjar_backend::services::stellar_service::StellarService;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a `StellarService` pointing at the given mock server base URL.
fn make_service(server: &MockServer) -> StellarService {
    StellarService::new(server.base_url(), "testnet".to_string())
        .with_horizon_url(server.base_url())
}

fn assert_stellar_code(err: &AppError, expected_code: &str) {
    match err {
        AppError::Stellar(s) => assert_eq!(
            s.code(),
            expected_code,
            "expected StellarError code '{}', got '{}'",
            expected_code,
            s.code()
        ),
        other => panic!("expected AppError::Stellar, got {:?}", other),
    }
}

// ── Memo validation ───────────────────────────────────────────────────────────

#[test]
fn memo_28_bytes_ascii_is_valid() {
    // Exactly 28 ASCII bytes — must pass.
    let memo = "a".repeat(28);
    assert_eq!(memo.len(), 28);
    StellarService::validate_memo(&memo).expect("28-byte ASCII memo should be valid");
}

#[test]
fn memo_29_bytes_ascii_is_rejected() {
    // 29 ASCII bytes — must fail.
    let memo = "a".repeat(29);
    assert_eq!(memo.len(), 29);
    let err = StellarService::validate_memo(&memo).unwrap_err();
    assert_stellar_code(&err, "STELLAR_MEMO_TOO_LONG");
    if let AppError::Stellar(StellarError::MemoTooLong { actual_bytes }) = err {
        assert_eq!(actual_bytes, 29);
    } else {
        panic!("wrong error variant");
    }
}

#[test]
fn memo_multibyte_emoji_at_boundary() {
    // "🎉" is 4 bytes.  7 × 4 = 28 bytes — should pass.
    let memo = "🎉".repeat(7);
    assert_eq!(memo.len(), 28, "7 emojis = 28 bytes");
    StellarService::validate_memo(&memo).expect("28-byte emoji memo should be valid");
}

#[test]
fn memo_multibyte_emoji_over_boundary() {
    // 8 × 4 = 32 bytes — must fail.
    let memo = "🎉".repeat(8);
    assert_eq!(memo.len(), 32, "8 emojis = 32 bytes");
    let err = StellarService::validate_memo(&memo).unwrap_err();
    assert_stellar_code(&err, "STELLAR_MEMO_TOO_LONG");
}

#[test]
fn memo_mixed_content_at_boundary() {
    // "hello🎉" = 5 + 4 = 9 bytes; repeat to hit exactly 27 bytes is complex.
    // Use a simpler boundary: 24 ASCII + 1 emoji (4 bytes) = 28 bytes.
    let memo = format!("{}{}", "x".repeat(24), "🎉");
    assert_eq!(memo.len(), 28);
    StellarService::validate_memo(&memo).expect("mixed 28-byte memo should be valid");
}

#[test]
fn memo_empty_is_valid() {
    StellarService::validate_memo("").expect("empty memo should be valid");
}

// ── Dynamic fee ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn fetch_base_fee_returns_p99_during_surge() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(GET).path("/fee_stats");
        then.status(200).json_body(json!({
            "last_ledger_base_fee": "100",
            "fee_charged": {
                "min": "100",
                "max": "5000",
                "p10": "100",
                "p20": "100",
                "p30": "200",
                "p40": "500",
                "p50": "1000",
                "p60": "2000",
                "p70": "3000",
                "p80": "4000",
                "p90": "4500",
                "p95": "4800",
                "p99": "5000"
            }
        }));
    });

    let svc = make_service(&server);
    let fee = svc.fetch_base_fee().await;
    assert_eq!(fee, 5000, "should return p99 during surge pricing");
}

#[tokio::test]
async fn fetch_base_fee_clamps_to_ceiling() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(GET).path("/fee_stats");
        then.status(200).json_body(json!({
            "fee_charged": { "p99": "999999" }  // way above ceiling
        }));
    });

    let svc = make_service(&server);
    let fee = svc.fetch_base_fee().await;
    assert_eq!(fee, 10_000, "should be clamped to FEE_CEILING_STROOPS");
}

#[tokio::test]
async fn fetch_base_fee_falls_back_to_floor_on_error() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(GET).path("/fee_stats");
        then.status(500);
    });

    let svc = make_service(&server);
    let fee = svc.fetch_base_fee().await;
    assert_eq!(fee, 100, "should fall back to FEE_FLOOR_STROOPS on error");
}

#[tokio::test]
async fn fetch_base_fee_falls_back_to_last_ledger_when_no_p99() {
    let server = MockServer::start();

    server.mock(|when, then| {
        when.method(GET).path("/fee_stats");
        then.status(200).json_body(json!({
            "last_ledger_base_fee": "200",
            "fee_charged": {}  // no p99
        }));
    });

    let svc = make_service(&server);
    let fee = svc.fetch_base_fee().await;
    assert_eq!(fee, 200);
}

// ── Destination account check ─────────────────────────────────────────────────

#[tokio::test]
async fn check_account_exists_returns_account_when_funded() {
    let server = MockServer::start();
    let address = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";

    server.mock(|when, then| {
        when.method(GET).path(format!("/accounts/{}", address));
        then.status(200).json_body(json!({
            "account_id": address,
            "subentry_count": 2,
            "balances": [{
                "asset_type": "native",
                "balance": "100.0000000",
                "selling_liabilities": "0.0000000"
            }]
        }));
    });

    let svc = make_service(&server);
    let account = svc.check_account_exists(address).await.unwrap();
    assert_eq!(account.account_id, address);
    assert_eq!(account.subentry_count, 2);
}

#[tokio::test]
async fn check_account_returns_destination_unfunded_for_404() {
    let server = MockServer::start();
    let address = "GBOGRANDNEWADDRESSNOTEXIST123456789012345678901234567890";

    server.mock(|when, then| {
        when.method(GET).path(format!("/accounts/{}", address));
        then.status(404).json_body(json!({
            "type": "https://stellar.org/horizon-errors/not_found",
            "title": "Resource Missing",
            "status": 404
        }));
    });

    let svc = make_service(&server);
    let err = svc.check_account_exists(address).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_DESTINATION_UNFUNDED");
}

// ── Spendable balance validation ──────────────────────────────────────────────

#[tokio::test]
async fn validate_balance_succeeds_when_sufficient() {
    let server = MockServer::start();
    let address = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";

    server.mock(|when, then| {
        when.method(GET).path(format!("/accounts/{}", address));
        then.status(200).json_body(json!({
            "account_id": address,
            "subentry_count": 0,
            // balance = 10 XLM; reserve = (2+0)*0.5 = 1 XLM; spendable = 9 XLM
            "balances": [{
                "asset_type": "native",
                "balance": "10.0000000",
                "selling_liabilities": "0.0000000"
            }]
        }));
    });

    let svc = make_service(&server);
    // Tip of 5 XLM is well within the 9 XLM spendable.
    svc.validate_spendable_balance(address, "5.0").await.unwrap();
}

#[tokio::test]
async fn validate_balance_rejects_when_below_reserve() {
    let server = MockServer::start();
    let address = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";

    server.mock(|when, then| {
        when.method(GET).path(format!("/accounts/{}", address));
        then.status(200).json_body(json!({
            "account_id": address,
            "subentry_count": 0,
            // balance = 1 XLM; reserve = 1 XLM; spendable = 0 XLM
            "balances": [{
                "asset_type": "native",
                "balance": "1.0000000",
                "selling_liabilities": "0.0000000"
            }]
        }));
    });

    let svc = make_service(&server);
    let err = svc
        .validate_spendable_balance(address, "0.5")
        .await
        .unwrap_err();
    assert_stellar_code(&err, "STELLAR_INSUFFICIENT_BALANCE");
}

#[tokio::test]
async fn validate_balance_accounts_for_selling_liabilities() {
    let server = MockServer::start();
    let address = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";

    server.mock(|when, then| {
        when.method(GET).path(format!("/accounts/{}", address));
        then.status(200).json_body(json!({
            "account_id": address,
            "subentry_count": 0,
            // balance=10, reserve=1, liabilities=8 → spendable=1 XLM
            "balances": [{
                "asset_type": "native",
                "balance": "10.0000000",
                "selling_liabilities": "8.0000000"
            }]
        }));
    });

    let svc = make_service(&server);
    // Tip of 5 XLM exceeds the 1 XLM spendable (after liabilities).
    let err = svc
        .validate_spendable_balance(address, "5.0")
        .await
        .unwrap_err();
    assert_stellar_code(&err, "STELLAR_INSUFFICIENT_BALANCE");
}

#[tokio::test]
async fn validate_balance_returns_unfunded_when_account_missing() {
    let server = MockServer::start();
    let address = "GBOGRANDNEWADDRESSNOTEXIST123456789012345678901234567890";

    server.mock(|when, then| {
        when.method(GET).path(format!("/accounts/{}", address));
        then.status(404);
    });

    let svc = make_service(&server);
    let err = svc
        .validate_spendable_balance(address, "1.0")
        .await
        .unwrap_err();
    assert_stellar_code(&err, "STELLAR_DESTINATION_UNFUNDED");
}

// ── verify_transaction: success paths ────────────────────────────────────────

#[tokio::test]
async fn verify_transaction_returns_true_for_successful_tx() {
    let server = MockServer::start();
    let hash = "a".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(200).json_body(json!({
            "id": hash,
            "hash": hash,
            "successful": true
        }));
    });

    let svc = make_service(&server);
    assert!(svc.verify_transaction(&hash).await.unwrap());
}

#[tokio::test]
async fn verify_transaction_returns_false_for_404() {
    let server = MockServer::start();
    let hash = "b".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(404);
    });

    let svc = make_service(&server);
    assert!(!svc.verify_transaction(&hash).await.unwrap());
}

// ── verify_transaction: Horizon error mapping ─────────────────────────────────

#[tokio::test]
async fn verify_transaction_maps_tx_bad_seq() {
    let server = MockServer::start();
    let hash = "c".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(200).json_body(json!({
            "id": hash, "hash": hash, "successful": false,
            "result_codes": { "transaction": "tx_bad_seq" }
        }));
    });

    let svc = make_service(&server);
    let err = svc.verify_transaction(&hash).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_TX_FAILED");
    if let AppError::Stellar(StellarError::TransactionFailed { code, message }) = err {
        assert_eq!(code, "tx_bad_seq");
        assert!(
            message.contains("Sequence number"),
            "message should mention sequence: {message}"
        );
    }
}

#[tokio::test]
async fn verify_transaction_maps_tx_insufficient_fee() {
    let server = MockServer::start();
    let hash = "d".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(200).json_body(json!({
            "id": hash, "hash": hash, "successful": false,
            "result_codes": { "transaction": "tx_insufficient_fee" }
        }));
    });

    let svc = make_service(&server);
    let err = svc.verify_transaction(&hash).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_TX_FAILED");
    if let AppError::Stellar(StellarError::TransactionFailed { message, .. }) = err {
        assert!(message.contains("surge"), "message should mention surge: {message}");
    }
}

#[tokio::test]
async fn verify_transaction_maps_op_underfunded() {
    let server = MockServer::start();
    let hash = "e".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(200).json_body(json!({
            "id": hash, "hash": hash, "successful": false,
            "result_codes": {
                "transaction": "tx_failed",
                "operations": ["op_underfunded"]
            }
        }));
    });

    let svc = make_service(&server);
    let err = svc.verify_transaction(&hash).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_OP_FAILED");
    if let AppError::Stellar(StellarError::OperationFailed { code, message }) = err {
        assert_eq!(code, "op_underfunded");
        assert!(message.contains("balance"), "message should mention balance: {message}");
    }
}

#[tokio::test]
async fn verify_transaction_maps_op_no_destination() {
    let server = MockServer::start();
    let hash = "f".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(200).json_body(json!({
            "id": hash, "hash": hash, "successful": false,
            "result_codes": {
                "transaction": "tx_failed",
                "operations": ["op_no_destination"]
            }
        }));
    });

    let svc = make_service(&server);
    let err = svc.verify_transaction(&hash).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_OP_FAILED");
    if let AppError::Stellar(StellarError::OperationFailed { code, .. }) = err {
        assert_eq!(code, "op_no_destination");
    }
}

// ── HTTP 429 rate-limit mapping ───────────────────────────────────────────────

#[tokio::test]
async fn verify_transaction_maps_429_rate_limited() {
    let server = MockServer::start();
    let hash = "9".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(429)
            .header("Retry-After", "30")
            .json_body(json!({ "title": "Rate Limit Exceeded" }));
    });

    let svc = make_service(&server);
    let err = svc.verify_transaction(&hash).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_RATE_LIMITED");
    if let AppError::Stellar(StellarError::RateLimited { retry_after_secs }) = err {
        assert_eq!(retry_after_secs, 30);
    }
}

// ── HTTP 504 gateway-timeout mapping ─────────────────────────────────────────

/// Verifies that `GatewayTimeout` is returned when Horizon responds with 504.
/// The retry mechanism will attempt once more (with a 3 s delay in production),
/// but since both attempts would hit 504 we just verify the final error type.
#[tokio::test]
async fn verify_transaction_maps_504_to_gateway_timeout() {
    // Set the retry delay to 0 ms so the test completes instantly.
    std::env::set_var("STELLAR_504_RETRY_DELAY_MS", "0");

    let server = MockServer::start();
    let hash = "0".repeat(64);

    server.mock(|when, then| {
        when.method(GET).path(format!("/transactions/{}", hash));
        then.status(504);
    });

    let svc = StellarService::new(server.base_url(), "testnet".to_string())
        .with_horizon_url(server.base_url());

    let err = svc.verify_transaction(&hash).await.unwrap_err();
    assert_stellar_code(&err, "STELLAR_GATEWAY_TIMEOUT");

    match err {
        AppError::Stellar(StellarError::GatewayTimeout) => {}
        other => panic!("expected GatewayTimeout, got {:?}", other),
    }

    std::env::remove_var("STELLAR_504_RETRY_DELAY_MS");
}

// ── Error message quality ─────────────────────────────────────────────────────

#[test]
fn error_messages_are_user_displayable() {
    let errors: Vec<StellarError> = vec![
        StellarError::MemoTooLong { actual_bytes: 32 },
        StellarError::DestinationUnfunded {
            address: "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN".to_string(),
        },
        StellarError::InsufficientBalance {
            available_xlm: "0.5000000".to_string(),
            required_xlm: "5.0010000".to_string(),
        },
        StellarError::TransactionFailed {
            code: "tx_bad_seq".to_string(),
            message: map_tx_result_code("tx_bad_seq").unwrap().to_string(),
        },
        StellarError::OperationFailed {
            code: "op_underfunded".to_string(),
            message: map_op_result_code("op_underfunded").unwrap().to_string(),
        },
        StellarError::RateLimited {
            retry_after_secs: 60,
        },
        StellarError::GatewayTimeout,
    ];

    for err in &errors {
        let msg = err.message();
        assert!(!msg.is_empty(), "error {} has empty message", err.code());
        // Message must not leak internal Rust types.
        assert!(
            !msg.contains("StellarError"),
            "message for {} leaks type name: {}",
            err.code(),
            msg
        );
        // details() must be valid JSON.
        let details = err.details();
        assert!(details.is_object(), "details for {} is not an object", err.code());
    }
}

// ── Import helpers at file scope ──────────────────────────────────────────────
use stellar_tipjar_backend::errors::stellar::{map_op_result_code, map_tx_result_code};
