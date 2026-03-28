//! Comprehensive test runner with coverage and performance metrics

use std::collections::HashMap;
use std::time::{Duration, Instant};

mod common;
mod helpers;

/// Test suite results
#[derive(Debug)]
pub struct TestSuiteResults {
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub total_duration: Duration,
    pub coverage_metrics: CoverageMetrics,
    pub performance_metrics: PerformanceMetrics,
}

/// Coverage metrics for different areas
#[derive(Debug)]
pub struct CoverageMetrics {
    pub api_endpoints_tested: usize,
    pub error_scenarios_tested: usize,
    pub edge_cases_tested: usize,
    pub concurrent_scenarios_tested: usize,
    pub performance_scenarios_tested: usize,
}

/// Performance metrics across all tests
#[derive(Debug)]
pub struct PerformanceMetrics {
    pub avg_response_time: Duration,
    pub max_response_time: Duration,
    pub min_response_time: Duration,
    pub throughput_ops_per_sec: f64,
    pub memory_usage_mb: Option<f64>,
}

impl TestSuiteResults {
    pub fn new() -> Self {
        Self {
            total_tests: 0,
            passed_tests: 0,
            failed_tests: 0,
            total_duration: Duration::from_secs(0),
            coverage_metrics: CoverageMetrics {
                api_endpoints_tested: 0,
                error_scenarios_tested: 0,
                edge_cases_tested: 0,
                concurrent_scenarios_tested: 0,
                performance_scenarios_tested: 0,
            },
            performance_metrics: PerformanceMetrics {
                avg_response_time: Duration::from_secs(0),
                max_response_time: Duration::from_secs(0),
                min_response_time: Duration::from_secs(60),
                throughput_ops_per_sec: 0.0,
                memory_usage_mb: None,
            },
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_tests == 0 {
            0.0
        } else {
            (self.passed_tests as f64 / self.total_tests as f64) * 100.0
        }
    }

    pub fn print_summary(&self) {
        println!("\n=== COMPREHENSIVE TEST SUITE RESULTS ===");
        println!("Total Tests: {}", self.total_tests);
        println!("Passed: {}", self.passed_tests);
        println!("Failed: {}", self.failed_tests);
        println!("Success Rate: {:.2}%", self.success_rate());
        println!("Total Duration: {:?}", self.total_duration);

        println!("\n=== COVERAGE METRICS ===");
        println!(
            "API Endpoints Tested: {}",
            self.coverage_metrics.api_endpoints_tested
        );
        println!(
            "Error Scenarios: {}",
            self.coverage_metrics.error_scenarios_tested
        );
        println!("Edge Cases: {}", self.coverage_metrics.edge_cases_tested);
        println!(
            "Concurrent Scenarios: {}",
            self.coverage_metrics.concurrent_scenarios_tested
        );
        println!(
            "Performance Scenarios: {}",
            self.coverage_metrics.performance_scenarios_tested
        );

        println!("\n=== PERFORMANCE METRICS ===");
        println!(
            "Average Response Time: {:?}",
            self.performance_metrics.avg_response_time
        );
        println!(
            "Max Response Time: {:?}",
            self.performance_metrics.max_response_time
        );
        println!(
            "Min Response Time: {:?}",
            self.performance_metrics.min_response_time
        );
        println!(
            "Throughput: {:.2} ops/sec",
            self.performance_metrics.throughput_ops_per_sec
        );

        if let Some(memory) = self.performance_metrics.memory_usage_mb {
            println!("Memory Usage: {:.2} MB", memory);
        }
    }
}

/// Individual test result
#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration: Duration,
    pub error_message: Option<String>,
}

/// Test category for organization
#[derive(Debug, Clone)]
pub enum TestCategory {
    BasicFunctionality,
    ErrorHandling,
    EdgeCases,
    Performance,
    Concurrency,
    Security,
    Integration,
}

/// Comprehensive test runner
pub struct ComprehensiveTestRunner {
    results: Vec<TestResult>,
    start_time: Option<Instant>,
}

impl ComprehensiveTestRunner {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            start_time: None,
        }
    }

    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
        println!("Starting comprehensive test suite...");
    }

    pub async fn run_test<F, Fut>(&mut self, name: &str, test_fn: F) -> bool
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
    {
        println!("Running test: {}", name);
        let start = Instant::now();

        match test_fn().await {
            Ok(()) => {
                let duration = start.elapsed();
                self.results.push(TestResult {
                    name: name.to_string(),
                    passed: true,
                    duration,
                    error_message: None,
                });
                println!("✓ {} ({}ms)", name, duration.as_millis());
                true
            }
            Err(e) => {
                let duration = start.elapsed();
                self.results.push(TestResult {
                    name: name.to_string(),
                    passed: false,
                    duration,
                    error_message: Some(e.to_string()),
                });
                println!("✗ {} ({}ms): {}", name, duration.as_millis(), e);
                false
            }
        }
    }

    pub fn generate_results(&self) -> TestSuiteResults {
        let total_tests = self.results.len();
        let passed_tests = self.results.iter().filter(|r| r.passed).count();
        let failed_tests = total_tests - passed_tests;

        let total_duration = if let Some(start) = self.start_time {
            start.elapsed()
        } else {
            Duration::from_secs(0)
        };

        let response_times: Vec<Duration> = self.results.iter().map(|r| r.duration).collect();
        let avg_response_time = if !response_times.is_empty() {
            response_times.iter().sum::<Duration>() / response_times.len() as u32
        } else {
            Duration::from_secs(0)
        };

        let max_response_time = response_times
            .iter()
            .max()
            .copied()
            .unwrap_or(Duration::from_secs(0));
        let min_response_time = response_times
            .iter()
            .min()
            .copied()
            .unwrap_or(Duration::from_secs(0));

        let throughput_ops_per_sec = if total_duration.as_secs_f64() > 0.0 {
            total_tests as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        TestSuiteResults {
            total_tests,
            passed_tests,
            failed_tests,
            total_duration,
            coverage_metrics: CoverageMetrics {
                api_endpoints_tested: self.count_tests_by_pattern("api_"),
                error_scenarios_tested: self.count_tests_by_pattern("error_"),
                edge_cases_tested: self.count_tests_by_pattern("edge_"),
                concurrent_scenarios_tested: self.count_tests_by_pattern("concurrent_"),
                performance_scenarios_tested: self.count_tests_by_pattern("performance_"),
            },
            performance_metrics: PerformanceMetrics {
                avg_response_time,
                max_response_time,
                min_response_time,
                throughput_ops_per_sec,
                memory_usage_mb: None, // Would need system monitoring to implement
            },
        }
    }

    fn count_tests_by_pattern(&self, pattern: &str) -> usize {
        self.results
            .iter()
            .filter(|r| r.name.contains(pattern))
            .count()
    }
}

#[tokio::test]
async fn run_comprehensive_test_suite() {
    let mut runner = ComprehensiveTestRunner::new();
    runner.start();

    // Basic functionality tests
    runner
        .run_test("api_creator_creation", || async {
            test_creator_creation().await
        })
        .await;

    runner
        .run_test("api_tip_recording", || async { test_tip_recording().await })
        .await;

    runner
        .run_test("api_tip_retrieval", || async { test_tip_retrieval().await })
        .await;

    // Error handling tests
    runner
        .run_test("error_invalid_creator_data", || async {
            test_invalid_creator_data().await
        })
        .await;

    runner
        .run_test("error_stellar_verification_failure", || async {
            test_stellar_verification_failure().await
        })
        .await;

    runner
        .run_test("error_duplicate_transaction", || async {
            test_duplicate_transaction().await
        })
        .await;

    // Edge case tests
    runner
        .run_test("edge_boundary_values", || async {
            test_boundary_values().await
        })
        .await;

    runner
        .run_test("edge_special_characters", || async {
            test_special_characters().await
        })
        .await;

    runner
        .run_test("edge_large_payloads", || async {
            test_large_payloads().await
        })
        .await;

    // Performance tests
    runner
        .run_test("performance_bulk_operations", || async {
            test_bulk_operations().await
        })
        .await;

    runner
        .run_test("performance_response_times", || async {
            test_response_times().await
        })
        .await;

    // Concurrency tests
    runner
        .run_test("concurrent_tip_creation", || async {
            test_concurrent_tip_creation().await
        })
        .await;

    runner
        .run_test("concurrent_creator_creation", || async {
            test_concurrent_creator_creation().await
        })
        .await;

    // Generate and print results
    let results = runner.generate_results();
    results.print_summary();

    // Assert overall success
    assert!(
        results.success_rate() >= 80.0,
        "Test suite success rate ({:.2}%) below acceptable threshold (80%)",
        results.success_rate()
    );
}

// Individual test implementations
async fn test_creator_creation() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;
    let creator = ctx
        .create_creator("test_creator", "GTEST123", "test@test.com")
        .await;
    assert_eq!(creator["username"], "test_creator");
    ctx.cleanup().await;
    Ok(())
}

async fn test_tip_recording() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = helpers::TestContext::new().await;
    ctx.create_creator("tip_creator", "GTIP123", "tip@test.com")
        .await;
    let response = ctx
        .record_tip_with_mock("tip_creator", "10.0", "TXTEST123", true)
        .await;
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    ctx.cleanup().await;
    Ok(())
}

async fn test_tip_retrieval() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = helpers::TestContext::new().await;
    ctx.create_creator("retrieve_creator", "GRETRIEVE123", "retrieve@test.com")
        .await;
    ctx.record_tip_with_mock("retrieve_creator", "15.0", "TXRETRIEVE123", true)
        .await;
    let tips = ctx.get_creator_tips("retrieve_creator").await;
    assert_eq!(tips.len(), 1);
    ctx.cleanup().await;
    Ok(())
}

async fn test_invalid_creator_data() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;
    let response = ctx
        .server
        .post("/creators")
        .json(&serde_json::json!({
            "username": "", // Invalid empty username
            "wallet_address": "GINVALID123",
            "email": "invalid@test.com"
        }))
        .await;
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    ctx.cleanup().await;
    Ok(())
}

async fn test_stellar_verification_failure() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = helpers::TestContext::new().await;
    ctx.create_creator("fail_creator", "GFAIL123", "fail@test.com")
        .await;
    let response = ctx
        .record_tip_with_mock("fail_creator", "10.0", "TXFAIL123", false)
        .await;
    assert_eq!(
        response.status(),
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );
    ctx.cleanup().await;
    Ok(())
}

async fn test_duplicate_transaction() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = helpers::TestContext::new().await;
    ctx.create_creator("dup_creator", "GDUP123", "dup@test.com")
        .await;

    // First tip
    let response1 = ctx
        .record_tip_with_mock("dup_creator", "10.0", "TXDUP123", true)
        .await;
    assert_eq!(response1.status(), axum::http::StatusCode::CREATED);

    // Duplicate tip
    let response2 = ctx
        .record_tip_with_mock("dup_creator", "15.0", "TXDUP123", true)
        .await;
    assert_eq!(response2.status(), axum::http::StatusCode::CONFLICT);

    ctx.cleanup().await;
    Ok(())
}

async fn test_boundary_values() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = helpers::TestContext::new().await;
    ctx.create_creator("boundary_creator", "GBOUNDARY123", "boundary@test.com")
        .await;

    // Test very small amount
    let response = ctx
        .record_tip_with_mock("boundary_creator", "0.0000001", "TXBOUNDARY123", true)
        .await;
    // Should either succeed or fail gracefully
    assert!(
        response.status() == axum::http::StatusCode::CREATED
            || response.status() == axum::http::StatusCode::BAD_REQUEST
    );

    ctx.cleanup().await;
    Ok(())
}

async fn test_special_characters() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;
    let response = ctx
        .server
        .post("/creators")
        .json(&serde_json::json!({
            "username": "user🎉",
            "wallet_address": "GSPECIAL123",
            "email": "special@test.com"
        }))
        .await;

    // Should handle special characters gracefully
    assert!(
        response.status() == axum::http::StatusCode::CREATED
            || response.status() == axum::http::StatusCode::BAD_REQUEST
    );

    ctx.cleanup().await;
    Ok(())
}

async fn test_large_payloads() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;
    let large_data = "x".repeat(1000);
    let response = ctx
        .server
        .post("/creators")
        .json(&serde_json::json!({
            "username": "large_user",
            "wallet_address": "GLARGE123",
            "email": "large@test.com",
            "extra_data": large_data
        }))
        .await;

    // Should handle large payloads appropriately
    assert!(
        response.status() == axum::http::StatusCode::CREATED
            || response.status() == axum::http::StatusCode::BAD_REQUEST
            || response.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE
    );

    ctx.cleanup().await;
    Ok(())
}

async fn test_bulk_operations() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;
    let start = Instant::now();

    // Create 10 creators
    for i in 0..10 {
        ctx.create_creator(
            &format!("bulk_creator_{}", i),
            &format!("GBULK{:03}", i),
            &format!("bulk_{}@test.com", i),
        )
        .await;
    }

    let duration = start.elapsed();
    assert!(
        duration < Duration::from_secs(5),
        "Bulk operations too slow"
    );

    ctx.cleanup().await;
    Ok(())
}

async fn test_response_times() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;

    let start = Instant::now();
    ctx.create_creator("perf_creator", "GPERF123", "perf@test.com")
        .await;
    let duration = start.elapsed();

    assert!(
        duration < Duration::from_millis(500),
        "Response time too slow"
    );

    ctx.cleanup().await;
    Ok(())
}

async fn test_concurrent_tip_creation() -> Result<(), Box<dyn std::error::Error>> {
    let mut ctx = helpers::TestContext::new().await;
    ctx.create_creator("concurrent_creator", "GCONC123", "concurrent@test.com")
        .await;

    let mut tasks = helpers::ConcurrentTestRunner::new();
    for i in 0..5 {
        let tx_hash = format!("TXCONC{:03}", i);
        ctx.stellar_mocks.mock_successful_transaction(&tx_hash);

        let server = ctx.server.clone();
        tasks.spawn(async move {
            let response = server
                .post("/tips")
                .json(&serde_json::json!({
                    "username": "concurrent_creator",
                    "amount": format!("{}.0", i + 1),
                    "transaction_hash": tx_hash
                }))
                .await;
            assert_eq!(response.status(), axum::http::StatusCode::CREATED);
        });
    }

    tasks.wait_all().await;

    let tips = ctx.get_creator_tips("concurrent_creator").await;
    assert_eq!(tips.len(), 5);

    ctx.cleanup().await;
    Ok(())
}

async fn test_concurrent_creator_creation() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = helpers::TestContext::new().await;

    let mut tasks = helpers::ConcurrentTestRunner::new();
    for i in 0..5 {
        let server = ctx.server.clone();
        tasks.spawn(async move {
            let response = server
                .post("/creators")
                .json(&serde_json::json!({
                    "username": format!("concurrent_creator_{}", i),
                    "wallet_address": format!("GCONC{:03}", i),
                    "email": format!("concurrent_{}@test.com", i)
                }))
                .await;
            assert_eq!(response.status(), axum::http::StatusCode::CREATED);
        });
    }

    tasks.wait_all().await;
    ctx.cleanup().await;
    Ok(())
}
