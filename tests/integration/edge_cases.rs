//! Edge case and boundary condition tests

use axum::http::StatusCode;
use serde_json::json;

mod common;
mod helpers;

use helpers::TestContext;
use helpers::test_data::{CreatorBuilder, TipBuilder, EdgeCaseData};

#[tokio::test]
async fn test_invalid_creator_data() {
    let ctx = TestContext::new().await;

    let invalid_creators = EdgeCaseData::invalid_creators();

    for (i, invalid_creator) in invalid_creators.iter().enumerate() {
        println!("Testing invalid creator case {}: {:?}", i, invalid_creator);
        
        let response = ctx.server
            .post("/creators")
            .json(invalid_creator)
            .await;

        // Should return 400 Bad Request for validation errors
        response.assert_status(StatusCode::BAD_REQUEST);
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_invalid_tip_data() {
    let mut ctx = TestContext::new().await;

    // Create a valid creator first
    ctx.create_creator("valid_creator", "GVALID123", "valid@test.com").await;

    let invalid_tips = EdgeCaseData::invalid_tips();

    for (i, invalid_tip) in invalid_tips.iter().enumerate() {
        println!("Testing invalid tip case {}: {:?}", i, invalid_tip);
        
        // Mock stellar verification for tips that have transaction hashes
        if let Some(tx_hash) = invalid_tip.get("transaction_hash") {
            if let Some(hash_str) = tx_hash.as_str() {
                if !hash_str.is_empty() {
                    ctx.stellar_mocks.mock_successful_transaction(hash_str);
                }
            }
        }

        let response = ctx.server
            .post("/tips")
            .json(invalid_tip)
            .await;

        // Should return 400 Bad Request or 404 Not Found for validation errors
        assert!(
            response.status() == StatusCode::BAD_REQUEST || 
            response.status() == StatusCode::NOT_FOUND ||
            response.status() == StatusCode::UNPROCESSABLE_ENTITY,
            "Expected 400, 404, or 422 for invalid tip case {}, got {}",
            i,
            response.status()
        );
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_boundary_values() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("boundary_creator", "GBOUND123", "boundary@test.com").await;

    let boundary_cases = EdgeCaseData::boundary_values();

    for (i, boundary_case) in boundary_cases.iter().enumerate() {
        println!("Testing boundary case {}: {:?}", i, boundary_case);
        
        if let Some(tx_hash) = boundary_case.get("transaction_hash") {
            if let Some(hash_str) = tx_hash.as_str() {
                ctx.stellar_mocks.mock_successful_transaction(hash_str);
            }
        }

        let response = ctx.server
            .post("/tips")
            .json(boundary_case)
            .await;

        // Boundary cases should either succeed or fail gracefully
        assert!(
            response.status() == StatusCode::CREATED || 
            response.status() == StatusCode::BAD_REQUEST ||
            response.status() == StatusCode::UNPROCESSABLE_ENTITY,
            "Boundary case {} returned unexpected status: {}",
            i,
            response.status()
        );
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_duplicate_creator_username() {
    let ctx = TestContext::new().await;

    // Create first creator
    let response1 = ctx.server
        .post("/creators")
        .json(&json!({
            "username": "duplicate_user",
            "wallet_address": "GDUP1123",
            "email": "dup1@test.com"
        }))
        .await;
    response1.assert_status(StatusCode::CREATED);

    // Try to create second creator with same username
    let response2 = ctx.server
        .post("/creators")
        .json(&json!({
            "username": "duplicate_user", // Same username
            "wallet_address": "GDUP2123",
            "email": "dup2@test.com"
        }))
        .await;

    // Should fail due to unique constraint
    response2.assert_status(StatusCode::CONFLICT);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_duplicate_creator_email() {
    let ctx = TestContext::new().await;

    // Create first creator
    let response1 = ctx.server
        .post("/creators")
        .json(&json!({
            "username": "user1",
            "wallet_address": "GDUP1123",
            "email": "duplicate@test.com"
        }))
        .await;
    response1.assert_status(StatusCode::CREATED);

    // Try to create second creator with same email
    let response2 = ctx.server
        .post("/creators")
        .json(&json!({
            "username": "user2",
            "wallet_address": "GDUP2123",
            "email": "duplicate@test.com" // Same email
        }))
        .await;

    // Should fail due to unique constraint
    response2.assert_status(StatusCode::CONFLICT);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_very_long_input_strings() {
    let ctx = TestContext::new().await;

    // Test very long username
    let long_username = "a".repeat(1000);
    let response = ctx.server
        .post("/creators")
        .json(&json!({
            "username": long_username,
            "wallet_address": "GLONG123",
            "email": "long@test.com"
        }))
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);

    // Test very long email
    let long_email = format!("{}@test.com", "a".repeat(1000));
    let response = ctx.server
        .post("/creators")
        .json(&json!({
            "username": "long_email_user",
            "wallet_address": "GLONG123",
            "email": long_email
        }))
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_special_characters_in_input() {
    let ctx = TestContext::new().await;

    let special_chars_cases = vec![
        // SQL injection attempt
        ("'; DROP TABLE creators; --", "GSQL123", "sql@test.com"),
        // XSS attempt
        ("<script>alert('xss')</script>", "GXSS123", "xss@test.com"),
        // Unicode characters
        ("用户名", "GUNI123", "unicode@test.com"),
        // Emoji
        ("user🎉", "GEMOJI123", "emoji@test.com"),
        // Null bytes
        ("user\0null", "GNULL123", "null@test.com"),
    ];

    for (username, wallet, email) in special_chars_cases {
        let response = ctx.server
            .post("/creators")
            .json(&json!({
                "username": username,
                "wallet_address": wallet,
                "email": email
            }))
            .await;

        // Should either succeed (if properly sanitized) or fail gracefully
        assert!(
            response.status() == StatusCode::CREATED || 
            response.status() == StatusCode::BAD_REQUEST,
            "Special character test failed for username: {}", username
        );
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_concurrent_creator_creation() {
    let ctx = TestContext::new().await;

    let mut handles = vec![];

    // Try to create multiple creators concurrently
    for i in 0..10 {
        let server = ctx.server.clone();
        let handle = tokio::spawn(async move {
            let response = server
                .post("/creators")
                .json(&json!({
                    "username": format!("concurrent_user_{}", i),
                    "wallet_address": format!("GCONC{:03}", i),
                    "email": format!("concurrent_{}@test.com", i)
                }))
                .await;
            
            response.assert_status(StatusCode::CREATED);
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_malformed_json_requests() {
    let ctx = TestContext::new().await;

    let malformed_requests = vec![
        // Invalid JSON
        r#"{"username": "test", "wallet_address": "GTEST123", "email": "test@test.com""#, // Missing closing brace
        // Empty JSON
        "{}",
        // Non-JSON string
        "not json at all",
        // Partial JSON
        r#"{"username": "test""#,
    ];

    for malformed_json in malformed_requests {
        let response = ctx.server
            .post("/creators")
            .header("content-type", "application/json")
            .text(malformed_json)
            .await;

        // Should return 400 Bad Request for malformed JSON
        response.assert_status(StatusCode::BAD_REQUEST);
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_missing_content_type() {
    let ctx = TestContext::new().await;

    let response = ctx.server
        .post("/creators")
        .text(r#"{"username": "test", "wallet_address": "GTEST123", "email": "test@test.com"}"#)
        .await;

    // Should handle missing content-type gracefully
    assert!(
        response.status() == StatusCode::BAD_REQUEST || 
        response.status() == StatusCode::UNSUPPORTED_MEDIA_TYPE
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_extremely_large_request_body() {
    let ctx = TestContext::new().await;

    // Create a very large JSON payload
    let large_data = "x".repeat(10_000_000); // 10MB string
    let large_json = json!({
        "username": "large_user",
        "wallet_address": "GLARGE123",
        "email": "large@test.com",
        "extra_data": large_data
    });

    let response = ctx.server
        .post("/creators")
        .json(&large_json)
        .await;

    // Should reject large payloads
    assert!(
        response.status() == StatusCode::PAYLOAD_TOO_LARGE || 
        response.status() == StatusCode::BAD_REQUEST
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_zero_and_negative_amounts() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("amount_test_creator", "GAMT123", "amount@test.com").await;

    let amount_cases = vec![
        ("0", StatusCode::BAD_REQUEST),           // Zero amount
        ("-1", StatusCode::BAD_REQUEST),          // Negative amount
        ("-0.01", StatusCode::BAD_REQUEST),       // Small negative
        ("0.0", StatusCode::BAD_REQUEST),         // Zero with decimal
        ("0.00000000", StatusCode::BAD_REQUEST),  // Zero with many decimals
    ];

    for (amount, expected_status) in amount_cases {
        let tx_hash = format!("TXAMT{}", amount.replace(".", "").replace("-", "NEG"));
        
        if expected_status == StatusCode::CREATED {
            ctx.stellar_mocks.mock_successful_transaction(&tx_hash);
        }

        let response = ctx.server
            .post("/tips")
            .json(&json!({
                "username": "amount_test_creator",
                "amount": amount,
                "transaction_hash": tx_hash
            }))
            .await;

        response.assert_status(expected_status);
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_precision_limits() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("precision_creator", "GPREC123", "precision@test.com").await;

    let precision_cases = vec![
        // Stellar supports 7 decimal places
        ("1.1234567", StatusCode::CREATED),      // Exactly 7 decimals
        ("1.12345678", StatusCode::BAD_REQUEST), // 8 decimals (too many)
        ("1.123456789", StatusCode::BAD_REQUEST), // 9 decimals (too many)
        ("999999999.9999999", StatusCode::CREATED), // Large number with max precision
    ];

    for (i, (amount, expected_status)) in precision_cases.iter().enumerate() {
        let tx_hash = format!("TXPREC{:03}", i);
        
        if *expected_status == StatusCode::CREATED {
            ctx.stellar_mocks.mock_successful_transaction(&tx_hash);
        }

        let response = ctx.server
            .post("/tips")
            .json(&json!({
                "username": "precision_creator",
                "amount": amount,
                "transaction_hash": tx_hash
            }))
            .await;

        response.assert_status(*expected_status);
    }

    ctx.cleanup().await;
}