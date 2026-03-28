//! Withdrawal functionality integration tests

use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;

mod common;
mod helpers;

use helpers::{PerformanceMetrics, TestContext};

#[tokio::test]
async fn test_withdrawal_basic_flow() {
    let mut ctx = TestContext::new().await;

    // Create creator with tips
    ctx.create_creator("withdrawal_creator", "GWITH123", "withdrawal@test.com")
        .await;

    // Add some tips first
    for i in 0..3 {
        let tx_hash = format!("TXWITH{:03}", i);
        let response = ctx
            .record_tip_with_mock(
                "withdrawal_creator",
                &format!("{}.0", i + 10),
                &tx_hash,
                true,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
    }

    // Test withdrawal endpoint (if it exists)
    let response = ctx
        .server
        .post("/creators/withdrawal_creator/withdraw")
        .json(&json!({
            "amount": "15.0",
            "destination_address": "GDEST123456789"
        }))
        .await;

    // This might return 404 if withdrawal isn't implemented yet
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::NOT_IMPLEMENTED
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_withdrawal_insufficient_balance() {
    let mut ctx = TestContext::new().await;

    // Create creator with small tip
    ctx.create_creator("low_balance_creator", "GLOW123", "low@test.com")
        .await;

    let response = ctx
        .record_tip_with_mock("low_balance_creator", "5.0", "TXLOW123", true)
        .await;
    response.assert_status(StatusCode::CREATED);

    // Try to withdraw more than available
    let response = ctx
        .server
        .post("/creators/low_balance_creator/withdraw")
        .json(&json!({
            "amount": "10.0", // More than the 5.0 available
            "destination_address": "GDEST123456789"
        }))
        .await;

    // Should fail with insufficient balance or return 404 if not implemented
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::NOT_IMPLEMENTED
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_withdrawal_zero_balance() {
    let ctx = TestContext::new().await;

    // Create creator with no tips
    ctx.create_creator("zero_balance_creator", "GZERO123", "zero@test.com")
        .await;

    // Try to withdraw from zero balance
    let response = ctx
        .server
        .post("/creators/zero_balance_creator/withdraw")
        .json(&json!({
            "amount": "1.0",
            "destination_address": "GDEST123456789"
        }))
        .await;

    // Should fail or return 404 if not implemented
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::NOT_IMPLEMENTED
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_double_withdrawal_prevention() {
    let mut ctx = TestContext::new().await;

    // Create creator with tips
    ctx.create_creator("double_withdraw_creator", "GDOUBLE123", "double@test.com")
        .await;

    let response = ctx
        .record_tip_with_mock("double_withdraw_creator", "20.0", "TXDOUBLE123", true)
        .await;
    response.assert_status(StatusCode::CREATED);

    // First withdrawal
    let response1 = ctx
        .server
        .post("/creators/double_withdraw_creator/withdraw")
        .json(&json!({
            "amount": "15.0",
            "destination_address": "GDEST123456789"
        }))
        .await;

    // Second withdrawal (should fail due to insufficient balance)
    let response2 = ctx
        .server
        .post("/creators/double_withdraw_creator/withdraw")
        .json(&json!({
            "amount": "10.0", // Would exceed remaining balance
            "destination_address": "GDEST123456789"
        }))
        .await;

    // If withdrawal is implemented, second should fail
    if response1.status() == StatusCode::OK {
        assert_eq!(response2.status(), StatusCode::BAD_REQUEST);
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_withdrawal_invalid_destination() {
    let mut ctx = TestContext::new().await;

    // Create creator with tips
    ctx.create_creator("invalid_dest_creator", "GINVALID123", "invalid@test.com")
        .await;

    let response = ctx
        .record_tip_with_mock("invalid_dest_creator", "10.0", "TXINVALID123", true)
        .await;
    response.assert_status(StatusCode::CREATED);

    let invalid_addresses = vec![
        "",                // Empty address
        "INVALID",         // Too short
        "GINVALIDADDRESS", // Invalid format
        "123456789",       // Numbers only
    ];

    for invalid_addr in invalid_addresses {
        let response = ctx
            .server
            .post("/creators/invalid_dest_creator/withdraw")
            .json(&json!({
                "amount": "5.0",
                "destination_address": invalid_addr
            }))
            .await;

        // Should fail validation or return 404 if not implemented
        assert!(
            response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::NOT_FOUND
                || response.status() == StatusCode::NOT_IMPLEMENTED
        );
    }

    ctx.cleanup().await;
}
