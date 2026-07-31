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

// ─────────────────────────── Test server construction ───────────────────────

/// Builds a `TestServer` on a real loopback HTTP transport with an injected
/// `ConnectInfo<SocketAddr>`, instead of axum-test's default in-process mock
/// transport.
///
/// The mock transport never populates `ConnectInfo`, which the governor
/// rate-limiter's `PeerIpKeyExtractor` (used by `create_app`'s write/read
/// limiter layers) requires to identify a client. Without it every request
/// fails closed with `GovernorError::UnableToExtractKey` — a 429 "Unable to
/// identify client." on the very first request, not real rate limiting.
pub fn test_server(app: Router) -> axum_test::TestServer {
    axum_test::TestServer::builder()
        .http_transport()
        .build(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .expect("failed to build TestServer with real HTTP transport")
}

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

    let idempotency = Arc::new(
        stellar_tipjar_backend::idempotency::IdempotencyService::new(
            pool.clone(),
            redis.clone(),
            stellar_tipjar_backend::idempotency::IdempotencyConfig::default(),
        ),
    );

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
        encryption: Arc::new(
            stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new(),
        ),
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
/// Resolves the Redis URL for tests from `REDIS_URL`, falling back to the
/// standard local default — mirrors how `setup_test_db` resolves
/// `TEST_DATABASE_URL`/`DATABASE_URL`.
pub fn test_redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

/// Create a test app backed by a real Redis connection (rate limiting,
/// session/token revocation, and idempotency all check Redis directly, so
/// mocking it out — like the other `create_test_app_*` factories do with
/// `redis: None` — would skip the exact behavior these tests exercise).
/// Returns `None` for the connection if `redis_url` is unreachable, so
/// degraded-mode tests can pass a deliberately bad URL.
pub async fn create_test_app_with_redis(
    pool: PgPool,
    redis_url: &str,
) -> (Router, Option<redis::aio::ConnectionManager>) {
    let redis = cache::redis_client::connect(redis_url).await;
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let moderation = Arc::new(ModerationService::new(pool.clone()));
    let (queue, _queue_rx) = VerificationQueue::new();

    let idempotency = Arc::new(
        stellar_tipjar_backend::idempotency::IdempotencyService::new(
            pool.clone(),
            redis.clone(),
            stellar_tipjar_backend::idempotency::IdempotencyConfig::default(),
        ),
    );

    let state = Arc::new(AppState {
        db: pool,
        verifier: MockTipVerifier::always_confirm(),
        stellar: Arc::new(StellarService::new(
            "https://horizon-testnet.stellar.org".to_string(),
            "testnet".to_string(),
        )),
        queue,
        performance,
        moderation,
        redis: redis.clone(),
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        encryption: Arc::new(
            stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new(),
        ),
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

    (create_app(state), redis)
}

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

    let idempotency = Arc::new(
        stellar_tipjar_backend::idempotency::IdempotencyService::new(
            pool.clone(),
            redis.clone(),
            stellar_tipjar_backend::idempotency::IdempotencyConfig::default(),
        ),
    );

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
        encryption: Arc::new(
            stellar_tipjar_backend::crypto::encryption::EncryptionKeyManager::new(),
        ),
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
