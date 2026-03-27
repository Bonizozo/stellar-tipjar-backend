//! Comprehensive tip flow integration tests

use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

mod common;
mod helpers;

use helpers::{TestContext, ConcurrentTestRunner, PerformanceMetrics};
use helpers::test_data::{CreatorBuilder, TipBuilder, generate_creators, generate_tips};

#[tokio::test]
async fn test_complete_tip_flow() {
    let mut ctx = TestContext::new().await;

    // Create a creator
    let creator = ctx.create_creator("tip_flow_creator", "GFLOW123", "flow@test.com").await;
    assert_eq!(creator["username"], "tip_flow_creator");

    // Record a successful tip
    let response = ctx.record_tip_with_mock(
        "tip_flow_creator",
        "25.5",
        "TXFLOW123",
        true
    ).await;

    response.assert_status(StatusCode::CREATED);
    let tip = response.json::<serde_json::Value>();
    assert_eq!(tip["amount"], "25.5");
    assert_eq!(tip["creator_username"], "tip_flow_creator");

    // Verify tip appears in creator's tip list
    let tips = ctx.get_creator_tips("tip_flow_creator").await;
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0]["amount"], "25.5");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_multiple_tips_same_creator() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("multi_tip_creator", "GMULTI123", "multi@test.com").await;

    // Record multiple tips
    let tip_amounts = vec!["10.0", "25.5", "5.75", "100.0"];
    for (i, amount) in tip_amounts.iter().enumerate() {
        let tx_hash = format!("TXMULTI{}", i);
        let response = ctx.record_tip_with_mock(
            "multi_tip_creator",
            amount,
            &tx_hash,
            true
        ).await;
        response.assert_status(StatusCode::CREATED);
    }

    // Verify all tips are recorded
    let tips = ctx.get_creator_tips("multi_tip_creator").await;
    assert_eq!(tips.len(), 4);

    // Verify amounts (order might vary)
    let recorded_amounts: Vec<String> = tips.iter()
        .map(|tip| tip["amount"].as_str().unwrap().to_string())
        .collect();
    
    for amount in &tip_amounts {
        assert!(recorded_amounts.contains(&amount.to_string()));
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_concurrent_tips() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("concurrent_creator", "GCONC123", "concurrent@test.com").await;

    // Prepare concurrent tip operations
    let mut runner = ConcurrentTestRunner::new();
    let tip_count = 10;

    for i in 0..tip_count {
        let tx_hash = format!("TXCONC{:03}", i);
        let amount = format!("{}.{:02}", i + 1, i * 10 % 100);
        
        // Mock the stellar transaction
        ctx.stellar_mocks.mock_successful_transaction(&tx_hash);
        
        let server = ctx.server.clone();
        runner.spawn(async move {
            let response = server
                .post("/tips")
                .json(&json!({
                    "username": "concurrent_creator",
                    "amount": amount,
                    "transaction_hash": tx_hash
                }))
                .await;
            
            // All should succeed
            response.assert_status(StatusCode::CREATED);
        });
    }

    // Wait for all concurrent operations to complete
    runner.wait_all().await;

    // Verify all tips were recorded
    let tips = ctx.get_creator_tips("concurrent_creator").await;
    assert_eq!(tips.len(), tip_count);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_with_stellar_verification_failure() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("fail_creator", "GFAIL123", "fail@test.com").await;

    // Record tip with failed stellar verification
    let response = ctx.record_tip_with_mock(
        "fail_creator",
        "10.0",
        "TXFAIL123",
        false // This will mock a failed transaction
    ).await;

    // Should return 422 for failed verification
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    // Verify no tip was recorded
    let tips = ctx.get_creator_tips("fail_creator").await;
    assert_eq!(tips.len(), 0);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_nonexistent_transaction() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("notx_creator", "GNOTX123", "notx@test.com").await;

    // Mock non-existent transaction
    ctx.stellar_mocks.mock_nonexistent_transaction("TXNOTEXIST123");

    let response = ctx.server
        .post("/tips")
        .json(&json!({
            "username": "notx_creator",
            "amount": "10.0",
            "transaction_hash": "TXNOTEXIST123"
        }))
        .await;

    // Should return 422 for non-existent transaction
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_stellar_network_timeout() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("timeout_creator", "GTIMEOUT123", "timeout@test.com").await;

    // Mock network timeout
    ctx.stellar_mocks.mock_network_timeout("TXTIMEOUT123");

    let response = ctx.server
        .post("/tips")
        .json(&json!({
            "username": "timeout_creator",
            "amount": "10.0",
            "transaction_hash": "TXTIMEOUT123"
        }))
        .await;

    // Should return 502 for network issues
    response.assert_status(StatusCode::BAD_GATEWAY);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_duplicate_transaction_hash() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("dup_creator", "GDUP123", "dup@test.com").await;

    // Record first tip
    let response1 = ctx.record_tip_with_mock(
        "dup_creator",
        "10.0",
        "TXDUP123",
        true
    ).await;
    response1.assert_status(StatusCode::CREATED);

    // Try to record second tip with same transaction hash
    let response2 = ctx.record_tip_with_mock(
        "dup_creator",
        "15.0",
        "TXDUP123", // Same hash
        true
    ).await;

    // Should fail due to unique constraint
    response2.assert_status(StatusCode::CONFLICT);

    // Verify only one tip was recorded
    let tips = ctx.get_creator_tips("dup_creator").await;
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0]["amount"], "10.0");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_performance() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("perf_creator", "GPERF123", "perf@test.com").await;

    // Measure tip recording performance
    let (response, duration) = ctx.measure_time(|| async {
        ctx.record_tip_with_mock(
            "perf_creator",
            "10.0",
            "TXPERF123",
            true
        ).await
    }).await;

    response.assert_status(StatusCode::CREATED);

    let metrics = PerformanceMetrics::new(duration);
    
    // Assert response time is under 1 second
    metrics.assert_response_time_under(Duration::from_secs(1));
    
    println!("Tip recording took: {:?}", duration);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_bulk_tip_processing() {
    let mut ctx = TestContext::new().await;

    // Create multiple creators
    let creators = ctx.create_creators(5).await;

    // Record tips for each creator
    for (i, creator) in creators.iter().enumerate() {
        let username = creator["username"].as_str().unwrap();
        
        // Record multiple tips per creator
        for j in 0..3 {
            let tx_hash = format!("TXBULK{}_{}", i, j);
            let amount = format!("{}.{}", j + 1, i * 10 + j);
            
            let response = ctx.record_tip_with_mock(
                username,
                &amount,
                &tx_hash,
                true
            ).await;
            response.assert_status(StatusCode::CREATED);
        }
    }

    // Verify each creator has 3 tips
    for creator in &creators {
        let username = creator["username"].as_str().unwrap();
        let tips = ctx.get_creator_tips(username).await;
        assert_eq!(tips.len(), 3);
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_data_integrity() {
    let mut ctx = TestContext::new().await;

    // Create creator
    let creator = ctx.create_creator("integrity_creator", "GINT123", "integrity@test.com").await;
    let creator_id = creator["id"].as_str().unwrap();

    // Record tip
    let response = ctx.record_tip_with_mock(
        "integrity_creator",
        "42.75",
        "TXINT123",
        true
    ).await;
    response.assert_status(StatusCode::CREATED);

    let tip = response.json::<serde_json::Value>();

    // Verify all tip fields are correctly set
    assert_eq!(tip["creator_username"], "integrity_creator");
    assert_eq!(tip["amount"], "42.75");
    assert_eq!(tip["transaction_hash"], "TXINT123");
    assert!(tip["id"].is_string());
    assert!(tip["created_at"].is_string());

    // Verify tip is linked to correct creator
    let tips = ctx.get_creator_tips("integrity_creator").await;
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0]["creator_username"], "integrity_creator");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_ordering() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("order_creator", "GORDER123", "order@test.com").await;

    // Record tips with delays to ensure different timestamps
    let tip_data = vec![
        ("TXORDER1", "10.0"),
        ("TXORDER2", "20.0"),
        ("TXORDER3", "30.0"),
    ];

    for (tx_hash, amount) in &tip_data {
        let response = ctx.record_tip_with_mock(
            "order_creator",
            amount,
            tx_hash,
            true
        ).await;
        response.assert_status(StatusCode::CREATED);
        
        // Small delay to ensure different timestamps
        sleep(Duration::from_millis(10)).await;
    }

    // Get tips and verify ordering (should be newest first)
    let tips = ctx.get_creator_tips("order_creator").await;
    assert_eq!(tips.len(), 3);

    // Verify tips are ordered by creation time (newest first)
    for i in 0..tips.len() - 1 {
        let current_time = tips[i]["created_at"].as_str().unwrap();
        let next_time = tips[i + 1]["created_at"].as_str().unwrap();
        assert!(current_time >= next_time, "Tips should be ordered by creation time (newest first)");
    }

    ctx.cleanup().await;
}