//! Test helpers and utilities for integration tests

use axum_test::TestServer;
use httpmock::prelude::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub mod stellar_mock;
pub mod test_data;

/// Test context containing server, database, and mock services
pub struct TestContext {
    pub server: Arc<TestServer>,
    pub pool: PgPool,
    pub mock_server: Arc<MockServer>,
    pub stellar_mocks: StellarMocks,
    /// Unique per-context suffix. Every test in this suite runs against the
    /// same database, and cargo runs test functions (and test binaries) in
    /// parallel, so any fixed username collides with a concurrently running
    /// test on `creators.username UNIQUE`. Scoping names per context keeps
    /// tests independent of each other's data and of leftovers from a
    /// previous run.
    pub namespace: String,
}

/// Stellar API mock handlers. Each `mock_*` method just registers an
/// expectation on the shared mock server as a side effect — none of the
/// call sites across the test suite use the returned handle, so there's no
/// need to store it (a `Mock<'a>` borrows the server it came from, which
/// would make storing it alongside that same server in this struct a
/// self-referential-struct problem for no actual benefit).
pub struct StellarMocks {
    pub mock_server: Arc<MockServer>,
}

impl StellarMocks {
    pub fn new(mock_server: Arc<MockServer>) -> Self {
        Self { mock_server }
    }

    /// Mock a successful transaction verification
    pub fn mock_successful_transaction(&self, tx_hash: &str) {
        self.mock_server.mock(|when, then| {
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
    }

    /// Mock a failed transaction verification
    pub fn mock_failed_transaction(&self, tx_hash: &str) {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(200).json_body(json!({
                "id": tx_hash,
                "hash": tx_hash,
                "successful": false
            }));
        });
    }

    /// Mock a non-existent transaction
    pub fn mock_nonexistent_transaction(&self, tx_hash: &str) {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(404).json_body(json!({
                "type": "https://stellar.org/horizon-errors/not_found",
                "title": "Resource Missing",
                "status": 404
            }));
        });
    }

    /// Mock Stellar network timeout
    pub fn mock_network_timeout(&self, tx_hash: &str) {
        self.mock_server.mock(|when, then| {
            when.method(GET).path(format!("/transactions/{}", tx_hash));
            then.status(500).delay(Duration::from_secs(30)); // Simulate timeout
        });
    }
}

impl TestContext {
    /// Create a new test context with database, server, and mocks
    pub async fn new() -> Self {
        let pool = crate::common::setup_test_db().await;
        let mock_server = Arc::new(MockServer::start());
        let stellar_mocks = StellarMocks::new(mock_server.clone());

        // Create app with mocked stellar service
        let (app, _) =
            crate::common::create_test_app_with_mock_stellar(pool.clone(), &mock_server.base_url())
                .await;

        let server = Arc::new(crate::common::test_server(app));

        Self {
            server,
            pool,
            mock_server,
            stellar_mocks,
            namespace: Uuid::new_v4().simple().to_string()[..8].to_string(),
        }
    }

    /// Qualify `base` with this context's namespace, yielding a name no other
    /// concurrently running test will use. Look-ups must use the same scoped
    /// name the record was created with.
    pub fn scoped(&self, base: &str) -> String {
        format!("{}_{}", base, self.namespace)
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
                    &self.scoped(&format!("creator_{}", i)),
                    &format!("WALLET{:03}", i),
                    &format!("creator_{}_{}@test.com", i, self.namespace),
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
    /// Remove only the rows this context created.
    ///
    /// This deliberately does not call `common::cleanup_test_db`, which
    /// `TRUNCATE`s the shared tables: cargo runs the test functions in a
    /// binary on parallel threads against one database, so a global truncate
    /// deletes every concurrently running test's data out from under it. That
    /// is what made this suite fail — a neighbouring test would wipe the
    /// creator another test had just inserted, and the retry then collided on
    /// `creators.username UNIQUE`. Scoping the delete to this context's
    /// namespace lets the tests keep running in parallel.
    pub async fn cleanup(&self) {
        let scoped = format!("%_{}", self.namespace);
        sqlx::query(
            "DELETE FROM tips WHERE creator_username IN              (SELECT username FROM creators WHERE username LIKE $1)",
        )
        .bind(&scoped)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM creators WHERE username LIKE $1")
            .bind(&scoped)
            .execute(&self.pool)
            .await
            .unwrap();
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

/// Concurrent test utilities.
///
/// Polls tasks concurrently via `join_all` rather than `tokio::spawn`, since
/// `TestServer`'s request future isn't `Send` (axum-test's internal cookie-jar
/// state isn't thread-safe) — `tokio::spawn` requires `Send` because it can
/// hop the future across OS threads, but `join_all` just interleaves polling
/// on the current task, which is all these tests actually need to exercise
/// the app's own concurrency handling (e.g. DB unique-constraint races).
pub struct ConcurrentTestRunner {
    pub tasks: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>>,
}

impl ConcurrentTestRunner {
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        self.tasks.push(Box::pin(future));
    }

    pub async fn wait_all(self) {
        futures::future::join_all(self.tasks).await;
    }
}

/// Test data generators
pub fn generate_test_wallet() -> String {
    format!(
        "G{}",
        &uuid::Uuid::new_v4()
            .to_string()
            .replace("-", "")
            .to_uppercase()[..55]
    )
}

pub fn generate_test_tx_hash() -> String {
    format!("TX{}", uuid::Uuid::new_v4().to_string().replace("-", ""))
}

pub fn generate_test_email() -> String {
    format!("test_{}@example.com", uuid::Uuid::new_v4())
}
