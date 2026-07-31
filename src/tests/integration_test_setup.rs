// Utility to build the Axum app for integration tests using the test configuration.
//
// Delegates the actual router assembly to `crate::create_app`, the same
// factory production uses, so these tests exercise the real route/middleware
// stack instead of a hand-rolled subset that drifts out of sync with it.

use crate::db::connection::{connect_with_retry, AppState};
use crate::idempotency;
use crate::queue;
use crate::services;
use axum::Router;
use dotenvy::dotenv;
use std::sync::Arc;
use std::time::Duration;

pub async fn build_app() -> Router {
    // Load test env variables
    dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = connect_with_retry(
        &database_url,
        5, // max connections
        1, // min connections
        Duration::from_secs(1),
        2,  // max retries
        2,  // circuit breaker threshold
        30, // circuit breaker recovery secs
    )
    .await
    .expect("Failed to connect to DB");

    // Run migrations (assumes migrations folder present)
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Migrations failed");

    let stellar = Arc::new(services::stellar_service::StellarService::new(
        std::env::var("STELLAR_RPC_URL").unwrap_or_default(),
        std::env::var("STELLAR_NETWORK").unwrap_or_default(),
    ));
    let (queue, _rx) = queue::VerificationQueue::new();
    let performance = Arc::new(crate::db::performance::PerformanceMonitor::new());
    let redis = None; // not needed for current tests
    let broadcast_tx = tokio::sync::broadcast::channel(16).0;
    let (ws_shutdown_tx, _) = tokio::sync::watch::channel(false);
    let moderation = Arc::new(crate::moderation::ModerationService::new(pool.clone()));
    let idempotency = Arc::new(idempotency::IdempotencyService::new(
        pool.clone(),
        redis.clone(),
        idempotency::IdempotencyConfig::default(),
    ));
    let state = Arc::new(AppState {
        db: pool.clone(),
        verifier: stellar.clone(),
        stellar,
        queue,
        performance,
        redis,
        broadcast_tx,
        moderation,
        db_circuit_breaker: Arc::new(services::circuit_breaker::CircuitBreaker::new(
            5,
            Duration::from_secs(60),
        )),
        cache: None,
        invalidator: None,
        encryption: Arc::new(
            crate::crypto::encryption::EncryptionKeyManager::new()
                .load()
                .await
                .unwrap(),
        ),
        replicas: None,
        lock_service: None,
        ws_shutdown_tx,
        ws_config: crate::ws::WsConfig::from_env(),
        idempotency,
        sharding: None,
    });

    crate::create_app(state)
}
