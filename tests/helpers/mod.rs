//! Test helpers and utilities for integration tests

use axum_test::TestServer;
use httpmock::prelude::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub mod stellar_mock;
pub mod test_data;

/// Test context containing server, database, and mock services
pub struct TestContext {
    pub server: TestServer,
    pub pool: PgPool,
    pub mock_server: MockServer,
    pub stellar_mocks: StellarMocks,
}

/// Stellar API mock handlers
pub struct StellarMocks {
    pub mock_server: MockServer,
    pub transaction_mocks: HashMap<String, Mock>,
}

impl StellarMocks {
    pub fn new(mock_server: MockServer) -> Self {
        Self {
            mock_server,
            transaction_mocks: HashMap::new(),
        }
    }

    /// Mock a successful transaction verification
    pub fn mock_successful_transaction(&mut self, tx_hash: &str) -> &Mock {
        let mock = self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200).json_body(json!({
                "id": tx_hash,
                "hash": tx_hash,
                "successful": true,
                "source_account": "GABC123",
                "operations": [{
                    "type": "payment",
                    "amount": "10.0000000",
                    "asset_type": "native"
                }]
            }));
        });

        self.transaction_mocks.insert(tx_hash.to_string(), mock);
        self.transaction_mocks.get(tx_hash).unwrap()
    }

    /// Mock a failed transaction verification
    pub fn mock_failed_transaction(&mut self, tx_hash: &str) -> &Mock {
        let mock = self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200).json_body(json!({
                "id": tx_hash,
                "hash": tx_hash,
                "successful": false
            }));
        });

        self.transaction_mocks.insert(tx_hash.to_string(), mock);
        self.transaction_mocks.get(tx_hash).unwrap()
    }

    /// Mock a non-existent transaction
    pub fn mock_nonexistent_transaction(&mut self, tx_hash: &str) -> &Mock {
        let mock = self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(404).json_body(json!({
                "type": "https://stellar.org/horizon-errors/not_found",
                "title": "Resource Missing",
                "status": 404
            }));
        });

        self.transaction_mocks.insert(tx_hash.to_string(), mock);
        self.transaction_mocks.get(tx_hash).unwrap()
    }

    /// Mock Stellar network timeout
    pub fn mock_network_timeout(&mut self, tx_hash: &str) -> &Mock {
        let mock = self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(500).delay(Duration::from_secs(30)); // Simulate timeout
        });

        self.transaction_mocks.insert(tx_hash.to_string(), mock);
        self.transaction_mocks.get(tx_hash).unwrap()
    }
}

impl TestContext {
    /// Create a new test context with database, server, and mocks
    pub async fn new() -> Self {
        let pool = crate::common::setup_test_db().await;
        let mock_server = MockServer::start();
        let stellar_mocks = StellarMocks::new(mock_server.clone());

        // Create app with mocked stellar service
        let (app, _) =
            crate::common::create_test_app_with_mock_stellar(pool.clone(), &mock_server.base_url())
                .await;

        let server = TestServer::new(app).unwrap();

        Self {
            server,
            pool,
            mock_server,
            stellar_mocks,
        }
    }

    /// Create a test creator and return the response
    pub async fn create_creator(&self, username: &str, wallet: &str, email: &str) -> Value {
        let response = self
            .server
            .post("/creators")
            .json(&json!({
                "username": username,
                "wallet_address": wallet,
                "email": email
            }))
            .await;

        response.assert_status_ok();
        response.json()
    }

    /// Create multiple test creators
    pub async fn create_creators(&self, count: usize) -> Vec<Value> {
        let mut creators = Vec::new();
        for i in 0..count {
            let creator = self
                .create_creator(
                    &format!("creator_{}", i),
                    &format!("WALLET{:03}", i),
                    &format!("creator_{}@test.com", i),
                )
                .await;
            creators.push(creator);
        }
        creators
    }

    /// Record a tip with mocked stellar verification
    pub async fn record_tip_with_mock(
        &mut self,
        username: &str,
        amount: &str,
        tx_hash: &str,
        should_succeed: bool,
    ) -> axum_test::TestResponse {
        if should_succeed {
            self.stellar_mocks.mock_successful_transaction(tx_hash);
        } else {
            self.stellar_mocks.mock_failed_transaction(tx_hash);
        }

        self.server
            .post("/tips")
            .json(&json!({
                "username": username,
                "amount": amount,
                "transaction_hash": tx_hash
            }))
            .await
    }

    /// Insert tip directly into database (bypassing stellar verification)
    pub async fn insert_tip_direct(
        &self,
        creator_username: &str,
        amount: &str,
        tx_hash: &str,
    ) -> Uuid {
        let tip_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO tips (id, creator_username, amount, transaction_hash, created_at) 
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(tip_id)
        .bind(creator_username)
        .bind(amount)
        .bind(tx_hash)
        .execute(&self.pool)
        .await
        .unwrap();

        tip_id
    }

    /// Get tips for a creator
    pub async fn get_creator_tips(&self, username: &str) -> Vec<Value> {
        let response = self
            .server
            .get(&format!("/creators/{}/tips", username))
            .await;

        response.assert_status_ok();
        response.json()
    }

    /// Measure execution time of an async operation
    pub async fn measure_time<F, Fut, T>(&self, operation: F) -> (T, Duration)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let start = Instant::now();
        let result = operation().await;
        let duration = start.elapsed();
        (result, duration)
    }

    /// Clean up test data
    pub async fn cleanup(&self) {
        crate::common::cleanup_test_db(&self.pool).await;
    }
}

/// Performance measurement utilities
pub struct PerformanceMetrics {
    pub response_time: Duration,
    pub database_queries: usize,
    pub memory_usage: Option<usize>,
}

impl PerformanceMetrics {
    pub fn new(response_time: Duration) -> Self {
        Self {
            response_time,
            database_queries: 0,
            memory_usage: None,
        }
    }

    pub fn assert_response_time_under(&self, max_duration: Duration) {
        assert!(
            self.response_time < max_duration,
            "Response time {:?} exceeded maximum {:?}",
            self.response_time,
            max_duration
        );
    }
}

/// Concurrent test utilities
pub struct ConcurrentTestRunner {
    pub tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl ConcurrentTestRunner {
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        self.tasks.push(handle);
    }

    pub async fn wait_all(self) {
        for task in self.tasks {
            task.await.unwrap();
        }
    }
}

/// Test data generators
pub fn generate_test_wallet() -> String {
    format!(
        "G{}",
        uuid::Uuid::new_v4()
            .to_string()
            .replace("-", "")
            .to_uppercase()[..55]
            .to_string()
    )
}

pub fn generate_test_tx_hash() -> String {
    format!("TX{}", uuid::Uuid::new_v4().to_string().replace("-", ""))
}

pub fn generate_test_email() -> String {
    format!("test_{}@example.com", uuid::Uuid::new_v4())
}
