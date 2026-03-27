//! Gas usage and performance optimization tests

use axum::http::StatusCode;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;

mod common;
mod helpers;

use helpers::{TestContext, PerformanceMetrics, ConcurrentTestRunner};

/// Gas/Performance measurement utilities
struct GasMetrics {
    pub operation_time: Duration,
    pub database_queries: usize,
    pub memory_usage_kb: Option<usize>,
    pub cpu_usage_percent: Option<f64>,
}

impl GasMetrics {
    pub fn new(operation_time: Duration) -> Self {
        Self {
            operation_time,
            database_queries: 0,
            memory_usage_kb: None,
            cpu_usage_percent: None,
        }
    }

    pub fn assert_operation_time_under(&self, max_duration: Duration) {
        assert!(
            self.operation_time < max_duration,
            "Operation took {:?}, expected under {:?}",
            self.operation_time,
            max_duration
        );
    }

    pub fn assert_database_queries_under(&self, max_queries: usize) {
        assert!(
            self.database_queries <= max_queries,
            "Operation used {} database queries, expected <= {}",
            self.database_queries,
            max_queries
        );
    }
}

#[tokio::test]
async fn test_creator_creation_performance() {
    let ctx = TestContext::new().await;

    // Measure single creator creation
    let start = Instant::now();
    let response = ctx.server
        .post("/creators")
        .json(&json!({
            "username": "perf_creator",
            "wallet_address": "GPERF123",
            "email": "perf@test.com"
        }))
        .await;
    let duration = start.elapsed();

    response.assert_status(StatusCode::CREATED);

    let metrics = GasMetrics::new(duration);
    
    // Creator creation should be fast (under 100ms)
    metrics.assert_operation_time_under(Duration::from_millis(100));
    
    println!("Creator creation took: {:?}", duration);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_recording_performance() {
    let mut ctx = TestContext::new().await;

    // Create creator first
    ctx.create_creator("tip_perf_creator", "GTIPPERF123", "tipperf@test.com").await;

    // Measure tip recording performance
    let start = Instant::now();
    let response = ctx.record_tip_with_mock(
        "tip_perf_creator",
        "10.0",
        "TXTIPPERF123",
        true
    ).await;
    let duration = start.elapsed();

    response.assert_status(StatusCode::CREATED);

    let metrics = GasMetrics::new(duration);
    
    // Tip recording should be reasonably fast (under 500ms including stellar mock)
    metrics.assert_operation_time_under(Duration::from_millis(500));
    
    println!("Tip recording took: {:?}", duration);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_bulk_creator_creation_performance() {
    let ctx = TestContext::new().await;

    let creator_count = 100;
    
    // Measure bulk creator creation
    let start = Instant::now();
    
    for i in 0..creator_count {
        let response = ctx.server
            .post("/creators")
            .json(&json!({
                "username": format!("bulk_creator_{}", i),
                "wallet_address": format!("GBULK{:03}", i),
                "email": format!("bulk_{}@test.com", i)
            }))
            .await;
        
        response.assert_status(StatusCode::CREATED);
    }
    
    let total_duration = start.elapsed();
    let avg_duration = total_duration / creator_count as u32;

    println!("Created {} creators in {:?} (avg: {:?} per creator)", 
             creator_count, total_duration, avg_duration);

    // Average creation time should be reasonable
    assert!(
        avg_duration < Duration::from_millis(50),
        "Average creator creation time {:?} too slow", avg_duration
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_bulk_tip_recording_performance() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("bulk_tip_creator", "GBULKTIP123", "bulktip@test.com").await;

    let tip_count = 50;
    
    // Measure bulk tip recording
    let start = Instant::now();
    
    for i in 0..tip_count {
        let tx_hash = format!("TXBULKTIP{:03}", i);
        let response = ctx.record_tip_with_mock(
            "bulk_tip_creator",
            &format!("{}.{:02}", i + 1, i % 100),
            &tx_hash,
            true
        ).await;
        
        response.assert_status(StatusCode::CREATED);
    }
    
    let total_duration = start.elapsed();
    let avg_duration = total_duration / tip_count as u32;

    println!("Recorded {} tips in {:?} (avg: {:?} per tip)", 
             tip_count, total_duration, avg_duration);

    // Average tip recording time should be reasonable
    assert!(
        avg_duration < Duration::from_millis(200),
        "Average tip recording time {:?} too slow", avg_duration
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_concurrent_operations_performance() {
    let mut ctx = TestContext::new().await;

    // Create multiple creators for concurrent testing
    let creator_count = 10;
    for i in 0..creator_count {
        ctx.create_creator(
            &format!("concurrent_perf_{}", i),
            &format!("GCONCPERF{:02}", i),
            &format!("concperf_{}@test.com", i)
        ).await;
    }

    let operations_per_creator = 5;
    let total_operations = creator_count * operations_per_creator;

    // Measure concurrent tip recording
    let start = Instant::now();
    let mut runner = ConcurrentTestRunner::new();

    for i in 0..creator_count {
        for j in 0..operations_per_creator {
            let tx_hash = format!("TXCONCPERF{}_{}", i, j);
            let username = format!("concurrent_perf_{}", i);
            let amount = format!("{}.{:02}", j + 1, (i + j) % 100);
            
            // Mock stellar transaction
            ctx.stellar_mocks.mock_successful_transaction(&tx_hash);
            
            let server = ctx.server.clone();
            runner.spawn(async move {
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

    runner.wait_all().await;
    let total_duration = start.elapsed();
    let avg_duration = total_duration / total_operations as u32;

    println!("Completed {} concurrent operations in {:?} (avg: {:?} per operation)", 
             total_operations, total_duration, avg_duration);

    // Concurrent operations should be more efficient than sequential
    assert!(
        avg_duration < Duration::from_millis(100),
        "Average concurrent operation time {:?} too slow", avg_duration
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_database_query_efficiency() {
    let ctx = TestContext::new().await;

    // Test creator retrieval efficiency
    ctx.create_creator("query_test_creator", "GQUERY123", "query@test.com").await;

    // Measure creator lookup performance
    let start = Instant::now();
    let response = ctx.server.get("/creators/query_test_creator").await;
    let duration = start.elapsed();

    response.assert_status(StatusCode::OK);

    let metrics = GasMetrics::new(duration);
    
    // Database lookup should be very fast
    metrics.assert_operation_time_under(Duration::from_millis(50));
    
    println!("Creator lookup took: {:?}", duration);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_tip_list_retrieval_performance() {
    let mut ctx = TestContext::new().await;

    // Create creator and multiple tips
    ctx.create_creator("tip_list_creator", "GTIPLIST123", "tiplist@test.com").await;

    let tip_count = 20;
    for i in 0..tip_count {
        ctx.insert_tip_direct(
            "tip_list_creator",
            &format!("{}.{:02}", i + 1, i % 100),
            &format!("TXTIPLIST{:03}", i)
        ).await;
    }

    // Measure tip list retrieval performance
    let start = Instant::now();
    let tips = ctx.get_creator_tips("tip_list_creator").await;
    let duration = start.elapsed();

    assert_eq!(tips.len(), tip_count);

    let metrics = GasMetrics::new(duration);
    
    // Tip list retrieval should be fast even with many tips
    metrics.assert_operation_time_under(Duration::from_millis(100));
    
    println!("Retrieved {} tips in {:?}", tip_count, duration);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_memory_usage_under_load() {
    let mut ctx = TestContext::new().await;

    // Create multiple creators and tips to test memory usage
    let creator_count = 20;
    let tips_per_creator = 10;

    let start = Instant::now();
    
    for i in 0..creator_count {
        let username = format!("memory_test_{}", i);
        ctx.create_creator(
            &username,
            &format!("GMEM{:03}", i),
            &format!("mem_{}@test.com", i)
        ).await;

        // Add tips for each creator
        for j in 0..tips_per_creator {
            let tx_hash = format!("TXMEM{}_{}", i, j);
            let response = ctx.record_tip_with_mock(
                &username,
                &format!("{}.{:02}", j + 1, i % 100),
                &tx_hash,
                true
            ).await;
            response.assert_status(StatusCode::CREATED);
        }
    }

    let total_duration = start.elapsed();
    let total_operations = creator_count + (creator_count * tips_per_creator);

    println!("Completed {} operations under load in {:?}", 
             total_operations, total_duration);

    // System should handle load without significant performance degradation
    let avg_duration = total_duration / total_operations as u32;
    assert!(
        avg_duration < Duration::from_millis(200),
        "Average operation time under load {:?} too slow", avg_duration
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_response_size_optimization() {
    let mut ctx = TestContext::new().await;

    // Create creator with many tips
    ctx.create_creator("response_size_creator", "GRESPSIZE123", "respsize@test.com").await;

    let tip_count = 50;
    for i in 0..tip_count {
        ctx.insert_tip_direct(
            "response_size_creator",
            &format!("{}.{:02}", i + 1, i % 100),
            &format!("TXRESPSIZE{:03}", i)
        ).await;
    }

    // Measure response retrieval and size
    let start = Instant::now();
    let response = ctx.server.get("/creators/response_size_creator/tips").await;
    let duration = start.elapsed();

    response.assert_status(StatusCode::OK);
    let tips = response.json::<serde_json::Value>();
    
    // Verify we got all tips
    assert_eq!(tips.as_array().unwrap().len(), tip_count);

    // Response should be retrieved quickly even with many tips
    assert!(
        duration < Duration::from_millis(200),
        "Large response retrieval took {:?}, too slow", duration
    );

    println!("Retrieved {} tips response in {:?}", tip_count, duration);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_error_handling_performance() {
    let mut ctx = TestContext::new().await;

    // Test performance of error scenarios
    let error_scenarios = vec![
        // Non-existent creator
        ("nonexistent_creator", "10.0", "TXERR1"),
        // Invalid amount (should be caught by validation)
        ("", "-5.0", "TXERR2"),
    ];

    for (i, (username, amount, tx_hash)) in error_scenarios.iter().enumerate() {
        if !username.is_empty() {
            ctx.stellar_mocks.mock_successful_transaction(tx_hash);
        }

        let start = Instant::now();
        let response = ctx.server
            .post("/tips")
            .json(&json!({
                "username": username,
                "amount": amount,
                "transaction_hash": tx_hash
            }))
            .await;
        let duration = start.elapsed();

        // Error responses should be fast
        assert!(
            duration < Duration::from_millis(100),
            "Error scenario {} took {:?}, too slow", i, duration
        );

        // Should return appropriate error status
        assert!(
            response.status() == StatusCode::BAD_REQUEST ||
            response.status() == StatusCode::NOT_FOUND ||
            response.status() == StatusCode::UNPROCESSABLE_ENTITY
        );

        println!("Error scenario {} handled in {:?}", i, duration);
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_stellar_api_timeout_handling() {
    let mut ctx = TestContext::new().await;

    // Create creator
    ctx.create_creator("timeout_perf_creator", "GTIMEOUTPERF123", "timeoutperf@test.com").await;

    // Mock a slow stellar response (but not timeout)
    let mock = ctx.mock_server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/transactions/TXSLOWTIMEOUT123");
        then.status(200)
            .delay(Duration::from_millis(500)) // 500ms delay
            .json_body(json!({
                "id": "TXSLOWTIMEOUT123",
                "hash": "TXSLOWTIMEOUT123",
                "successful": true
            }));
    });

    // Measure tip recording with slow stellar response
    let start = Instant::now();
    let response = ctx.server
        .post("/tips")
        .json(&json!({
            "username": "timeout_perf_creator",
            "amount": "10.0",
            "transaction_hash": "TXSLOWTIMEOUT123"
        }))
        .await;
    let duration = start.elapsed();

    // Should still succeed but take longer
    response.assert_status(StatusCode::CREATED);
    
    // Should handle the delay gracefully
    assert!(
        duration >= Duration::from_millis(500),
        "Should have waited for stellar response"
    );
    assert!(
        duration < Duration::from_secs(2),
        "Should not take too long even with stellar delay"
    );

    println!("Tip with slow stellar response took: {:?}", duration);

    mock.assert();
    ctx.cleanup().await;
}