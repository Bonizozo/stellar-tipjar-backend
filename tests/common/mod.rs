pub mod fixtures;

use async_trait::async_trait;
use axum::Router;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use stellar_tipjar_backend::db::connection::AppState;
use stellar_tipjar_backend::errors::AppResult;
use stellar_tipjar_backend::moderation::ModerationService;
use stellar_tipjar_backend::queue::VerificationQueue;
use stellar_tipjar_backend::services::stellar_service::{
    StellarService, TipVerifier, TipVerifyRequest, VerifyOutcome,
};
use stellar_tipjar_backend::{cache, create_app, db, email};

// ─────────────────────────── MockTipVerifier ────────────────────────────────

/// Programmable mock that can be pre-loaded with expected outcomes.
pub struct MockTipVerifier {
    /// Ordered list of outcomes to return, consumed one by one.
    outcomes: Mutex<Vec<AppResult<VerifyOutcome>>>,
}

impl MockTipVerifier {
    /// Create a verifier that always returns `Confirmed`.
    pub fn always_confirm() -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(vec![]),
        })
    }

    /// Create a verifier pre-loaded with a sequence of outcomes.
    pub fn with_outcomes(outcomes: Vec<AppResult<VerifyOutcome>>) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes),
        })
    }
}

#[async_trait]
impl TipVerifier for MockTipVerifier {
    async fn verify_tip(&self, _req: &TipVerifyRequest) -> AppResult<VerifyOutcome> {
        let mut queue = self.outcomes.lock().await;
        if queue.is_empty() {
            // Default: confirm unless instructed otherwise
            Ok(VerifyOutcome::Confirmed)
        } else {
            queue.remove(0)
        }
    }
}

// ─────────────────────────── DB helpers ─────────────────────────────────────

pub async fn setup_test_db() -> PgPool {
    dotenvy::from_filename(".env.test").ok();
    dotenvy::dotenv().ok(); // Fallback to .env

    let database_url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
        .await
        .unwrap();

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    pool
}

pub async fn cleanup_test_db(pool: &PgPool) {
    // Clean up in correct order due to foreign key constraints
    sqlx::query(
        "TRUNCATE campaign_matches, campaigns, notifications, notification_preferences, tips, creators, jobs CASCADE",
    )
    .execute(pool)
    .await
    .unwrap();
}

// ─────────────────────────── App factory ────────────────────────────────────

pub async fn create_test_app(pool: PgPool) -> (Router, String) {
    create_test_app_with_verifier(pool, MockTipVerifier::always_confirm()).await
}

pub async fn create_test_app_with_verifier(
    pool: PgPool,
    verifier: Arc<dyn TipVerifier>,
) -> (Router, String) {
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let moderation = Arc::new(ModerationService::new(pool.clone()));
    let redis = None;
    let (queue, _queue_rx) = VerificationQueue::new();
    // Note: we intentionally drop queue_rx here so the channel is effectively
    // a no-op in unit tests. The queue.enqueue() calls will succeed (channel not full),
    // but nothing processes them, which is fine for handler-level tests.

    let idempotency = Arc::new(stellar_tipjar_backend::idempotency::IdempotencyService::new(
        pool.clone(),
        redis.clone(),
        stellar_tipjar_backend::idempotency::IdempotencyConfig::default(),
    ));

    let state = Arc::new(AppState {
        db: pool,
        verifier,
        stellar: Arc::new(StellarService::new(
            "https://horizon-testnet.stellar.org".to_string(),
            "testnet".to_string(),
        )),
        queue,
        performance,
        moderation,
        redis,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        encryption: Arc::new(stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new()),
        replicas: None,
        db_circuit_breaker: Arc::new(
            stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(
                5,
                std::time::Duration::from_secs(60),
            ),
        ),
        lock_service: None,
        ws_shutdown_tx: tokio::sync::watch::channel(false).0,
        ws_config: stellar_tipjar_backend::ws::WsConfig::from_env(),
        idempotency,
        sharding: None,
    });

    (create_app(state), "mock_token".into())
}

/// Create a test app backed by a real StellarService pointing at a mock Horizon URL.
/// Useful for tests that want to exercise the full HTTP Horizon path without hitting
/// the live testnet.
pub async fn create_test_app_with_mock_stellar(
    pool: PgPool,
    mock_stellar_url: &str,
) -> (Router, String) {
    let stellar_network = "testnet".to_string();
    let stellar: Arc<dyn TipVerifier> = Arc::new(StellarService::new(
        mock_stellar_url.to_string(),
        stellar_network,
    ));
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let moderation = Arc::new(ModerationService::new(pool.clone()));
    let redis = None;
    let (queue, _queue_rx) = VerificationQueue::new();

    // Initialize email system (unused in tests but AppState may require it)
    let (email_sender, _email_rx) = email::sender::EmailSender::new();
    let _email_sender = Arc::new(email_sender);

    let idempotency = Arc::new(stellar_tipjar_backend::idempotency::IdempotencyService::new(
        pool.clone(),
        redis.clone(),
        stellar_tipjar_backend::idempotency::IdempotencyConfig::default(),
    ));

    let state = Arc::new(AppState {
        db: pool,
        verifier: stellar,
        stellar: Arc::new(StellarService::new(
            mock_stellar_url.to_string(),
            "testnet".to_string(),
        )),
        queue,
        performance,
        moderation,
        redis,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        encryption: Arc::new(stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new()),
        replicas: None,
        db_circuit_breaker: Arc::new(
            stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(
                5,
                std::time::Duration::from_secs(60),
            ),
        ),
        lock_service: None,
        ws_shutdown_tx: tokio::sync::watch::channel(false).0,
        ws_config: stellar_tipjar_backend::ws::WsConfig::from_env(),
        idempotency,
        sharding: None,
    });

    (create_app(state), "mock_token".into())
}
