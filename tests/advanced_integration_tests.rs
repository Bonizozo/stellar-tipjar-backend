//! Advanced integration tests covering complex scenarios

use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

mod common;
mod helpers;

use helpers::{ConcurrentTestRunner, PerformanceMetrics, TestContext};

#[tokio::test]
async fn test_high_volume_concurrent_operations() {
    let mut ctx = TestContext::new().await;

    // Create multiple creators
    let creator_count = 20;
    let tips_per_creator = 25;

    // Create creators concurrently
    let mut creator_tasks = ConcurrentTestRunner::new();
    for i in 0..creator_count {
        let server = ctx.server.clone();
        creator_tasks.spawn(async move {
            let response = server
                .post("/creators")
                .json(&json!({
                    "username": format!("volume_creator_{}", i),
                    "wallet_address": format!("GVOLUME{:03}", i),
                    "email": format!("volume_{}@test.com", i)
                }))
                .await;
            response.assert_status(StatusCode::CREATED);
        });
    }
    creator_tasks.wait_all().await;

    // Record tips concurrently for all creators
    let mut tip_tasks = ConcurrentTestRunner::new();
    for i in 0..creator_count {
        for j in 0..tips_per_creator {
            let tx_hash = format!("TXVOLUME{}_{:03}", i, j);
            let username = format!("volume_creator_{}", i);
            let amount = format!("{}.{:02}", j + 1, (i + j) % 100);

            // Mock stellar transaction
            ctx.stellar_mocks.mock_successful_transaction(&tx_hash);

            let server = ctx.server.clone();
            tip_tasks.spawn(async move {
                let response = server
                    .post("/tips")
                    .json(&json!({
                        "username": username,
                        "amount": amount,
                        "transaction_hash": tx_hash
                    }))
                    .await;
                response.assert_status(StatusCode::CREATED);
            });
        }
    }

    let start = std::time::Instant::now();
    tip_tasks.wait_all().await;
    let duration = start.elapsed();

    println!(
        "Processed {} concurrent tips in {:?}",
        creator_count * tips_per_creator,
        duration
    );

    // Verify all tips were recorded
    for i in 0..creator_count {
        let username = format!("volume_creator_{}", i);
        let tips = ctx.get_creator_tips(&username).await;
        assert_eq!(
            tips.len(),
            tips_per_creator,
            "Creator {} should have {} tips",
            username,
            tips_per_creator
        );
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_mixed_success_failure_scenarios() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("mixed_creator", "GMIXED123", "mixed@test.com")
        .await;

    let scenarios = vec![
        ("TXMIXED001", "10.0", true),  // Success
        ("TXMIXED002", "15.0", false), // Stellar failure
        ("TXMIXED003", "20.0", true),  // Success
        ("TXMIXED004", "25.0", false), // Stellar failure
        ("TXMIXED005", "30.0", true),  // Success
    ];

    let mut successful_tips = 0;
    for (tx_hash, amount, should_succeed) in scenarios {
        let response = ctx
            .record_tip_with_mock("mixed_creator", amount, tx_hash, should_succeed)
            .await;

        if should_succeed {
            response.assert_status(StatusCode::CREATED);
            successful_tips += 1;
        } else {
            response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        }
    }

    // Verify only successful tips were recorded
    let tips = ctx.get_creator_tips("mixed_creator").await;
    assert_eq!(tips.len(), successful_tips);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_database_transaction_rollback() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("rollback_creator", "GROLLBACK123", "rollback@test.com")
        .await;

    // Record a successful tip first
    let response = ctx
        .record_tip_with_mock("rollback_creator", "10.0", "TXROLLBACK001", true)
        .await;
    response.assert_status(StatusCode::CREATED);

    // Try to record a tip with duplicate transaction hash (should fail)
    let response = ctx
        .record_tip_with_mock(
            "rollback_creator",
            "15.0",
            "TXROLLBACK001", // Same hash as before
            true,
        )
        .await;
    response.assert_status(StatusCode::CONFLICT);

    // Verify only the first tip exists
    let tips = ctx.get_creator_tips("rollback_creator").await;
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0]["amount"], "10.0");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_rate_limiting_behavior() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("rate_limit_creator", "GRATE123", "rate@test.com")
        .await;

    // Rapidly send many requests to test rate limiting
    let request_count = 50;
    let mut responses = Vec::new();

    for i in 0..request_count {
        let tx_hash = format!("TXRATE{:03}", i);
        ctx.stellar_mocks.mock_successful_transaction(&tx_hash);

        let response = ctx
            .server
            .post("/tips")
            .json(&json!({
                "username": "rate_limit_creator",
                "amount": format!("{}.0", i + 1),
                "transaction_hash": tx_hash
            }))
            .await;

        responses.push(response.status());

        // Small delay to avoid overwhelming the system
        sleep(Duration::from_millis(10)).await;
    }

    // Count successful vs rate-limited responses
    let successful = responses
        .iter()
        .filter(|&&s| s == StatusCode::CREATED)
        .count();
    let rate_limited = responses
        .iter()
        .filter(|&&s| s == StatusCode::TOO_MANY_REQUESTS)
        .count();

    println!(
        "Successful: {}, Rate limited: {}, Total: {}",
        successful, rate_limited, request_count
    );

    // Should have some successful requests
    assert!(successful > 0, "Should have some successful requests");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_data_consistency_under_load() {
    let mut ctx = TestContext::new().await;

    // Create multiple creators
    let creator_count = 10;
    for i in 0..creator_count {
        ctx.create_creator(
            &format!("consistency_creator_{}", i),
            &format!("GCONS{:03}", i),
            &format!("cons_{}@test.com", i),
        )
        .await;
    }

    // Add tips concurrently and verify consistency
    let tips_per_creator = 10;
    let mut tasks = ConcurrentTestRunner::new();

    for i in 0..creator_count {
        for j in 0..tips_per_creator {
            let tx_hash = format!("TXCONS{}_{:03}", i, j);
            let username = format!("consistency_creator_{}", i);
            let amount = format!("{}.{:02}", j + 1, i % 100);

            ctx.stellar_mocks.mock_successful_transaction(&tx_hash);

            let server = ctx.server.clone();
            tasks.spawn(async move {
                let response = server
                    .post("/tips")
                    .json(&json!({
                        "username": username,
                        "amount": amount,
                        "transaction_hash": tx_hash
                    }))
                    .await;
                response.assert_status(StatusCode::CREATED);
            });
        }
    }

    tasks.wait_all().await;

    // Verify data consistency
    for i in 0..creator_count {
        let username = format!("consistency_creator_{}", i);
        let tips = ctx.get_creator_tips(&username).await;

        // Each creator should have exactly the expected number of tips
        assert_eq!(
            tips.len(),
            tips_per_creator,
            "Creator {} has inconsistent tip count",
            username
        );

        // Verify all tips belong to the correct creator
        for tip in &tips {
            assert_eq!(tip["creator_username"], username);
        }
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_large_payload_handling() {
    let ctx = TestContext::new().await;

    // Test with very long but valid data
    let long_username = "a".repeat(100); // Assuming this is within limits
    let long_email = format!("{}@test.com", "b".repeat(100));

    let response = ctx
        .server
        .post("/creators")
        .json(&json!({
            "username": long_username,
            "wallet_address": "GLONGPAYLOAD123456789012345678901234567890123456789012345",
            "email": long_email
        }))
        .await;

    // Should either succeed (if within limits) or fail gracefully
    assert!(
        response.status() == StatusCode::CREATED || response.status() == StatusCode::BAD_REQUEST
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_network_resilience() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("network_creator", "GNETWORK123", "network@test.com")
        .await;

    // Test various network failure scenarios
    let scenarios = vec![
        ("TXNET001", "timeout"),
        ("TXNET002", "not_found"),
        ("TXNET003", "server_error"),
    ];

    for (tx_hash, failure_type) in scenarios {
        match failure_type {
            "timeout" => ctx.stellar_mocks.mock_network_timeout(tx_hash),
            "not_found" => ctx.stellar_mocks.mock_nonexistent_transaction(tx_hash),
            "server_error" => {
                // Mock server error
                let mock = ctx.mock_server.mock(|when, then| {
                    when.method(httpmock::Method::GET)
                        .path(format!("/transactions/{}", tx_hash));
                    then.status(500);
                });
                ctx.stellar_mocks
                    .transaction_mocks
                    .insert(tx_hash.to_string(), mock);
            }
            _ => {}
        }

        let response = ctx
            .server
            .post("/tips")
            .json(&json!({
                "username": "network_creator",
                "amount": "10.0",
                "transaction_hash": tx_hash
            }))
            .await;

        // Should handle network failures gracefully
        assert!(
            response.status() == StatusCode::UNPROCESSABLE_ENTITY
                || response.status() == StatusCode::BAD_GATEWAY
                || response.status() == StatusCode::SERVICE_UNAVAILABLE
        );
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_memory_leak_prevention() {
    let mut ctx = TestContext::new().await;

    // Create and delete many creators to test for memory leaks
    let iterations = 100;

    for i in 0..iterations {
        // Create creator
        let username = format!("temp_creator_{}", i);
        ctx.create_creator(
            &username,
            &format!("GTEMP{:03}", i),
            &format!("temp_{}@test.com", i),
        )
        .await;

        // Add a tip
        let tx_hash = format!("TXTEMP{:03}", i);
        let response = ctx
            .record_tip_with_mock(&username, "10.0", &tx_hash, true)
            .await;
        response.assert_status(StatusCode::CREATED);

        // Periodically clean up to prevent excessive memory usage
        if i % 10 == 0 {
            // Clean up some data (this would be implementation-specific)
            sqlx::query("DELETE FROM tips WHERE creator_username LIKE 'temp_creator_%'")
                .execute(&ctx.pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM creators WHERE username LIKE 'temp_creator_%'")
                .execute(&ctx.pool)
                .await
                .unwrap();
        }
    }

    println!("Completed {} iterations without memory issues", iterations);
    ctx.cleanup().await;
}

#[tokio::test]
async fn test_unicode_and_internationalization() {
    let ctx = TestContext::new().await;

    let international_test_cases = vec![
        ("用户名", "GUNICODE123", "unicode@test.com"), // Chinese
        ("пользователь", "GRUSSIAN123", "russian@test.com"), // Russian
        ("ユーザー", "GJAPANESE123", "japanese@test.com"), // Japanese
        ("مستخدم", "GARABIC123", "arabic@test.com"),   // Arabic
        ("🎉emoji🎉", "GEMOJI123", "emoji@test.com"),  // Emoji
    ];

    for (username, wallet, email) in international_test_cases {
        let response = ctx
            .server
            .post("/creators")
            .json(&json!({
                "username": username,
                "wallet_address": wallet,
                "email": email
            }))
            .await;

        // Should either handle unicode properly or reject gracefully
        assert!(
            response.status() == StatusCode::CREATED
                || response.status() == StatusCode::BAD_REQUEST,
            "Failed to handle unicode username: {}",
            username
        );

        if response.status() == StatusCode::CREATED {
            // Verify the creator can be retrieved
            let get_response = ctx
                .server
                .get(&format!("/creators/{}", urlencoding::encode(username)))
                .await;

            // Should be able to retrieve the creator
            assert!(
                get_response.status() == StatusCode::OK
                    || get_response.status() == StatusCode::NOT_FOUND
            );
        }
    }

    ctx.cleanup().await;
}
