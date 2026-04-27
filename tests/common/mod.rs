pub mod fixtures;
use axum::Router;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;
use std::time::Duration;
use stellar_tipjar_backend::db::connection::AppState;
use stellar_tipjar_backend::moderation::ModerationService;
use stellar_tipjar_backend::services::stellar_service::StellarService;
use stellar_tipjar_backend::{cache, create_app, db, email};

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

pub async fn create_test_app(pool: PgPool) -> (Router, String) {
    let stellar_network = "testnet".to_string();
    // In actual tests, you'd use httpmock for stellar_rpc_url
    let stellar_rpc_url = "https://soroban-testnet.stellar.org".to_string();

    let stellar = StellarService::new(stellar_rpc_url, stellar_network);
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let moderation = Arc::new(ModerationService::new(pool.clone()));

    // Mock redis (or just let it fail/disable)
    let redis = None;

    // Initialize email system
    let (email_sender, _email_rx) = email::sender::EmailSender::new();
    let email_sender = Arc::new(email_sender);

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        moderation,
        redis,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
        lock_service: None,
    });

    (create_app(state), "mock_token".into())
}

pub async fn create_test_app_with_mock_stellar(
    pool: PgPool,
    mock_stellar_url: &str,
) -> (Router, String) {
    let stellar_network = "testnet".to_string();

    // Use the mock server URL for stellar service
    let stellar = StellarService::new(mock_stellar_url.to_string(), stellar_network);
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let moderation = Arc::new(ModerationService::new(pool.clone()));

    // Mock redis (or just let it fail/disable)
    let redis = None;

    // Initialize email system
    let (email_sender, _email_rx) = email::sender::EmailSender::new();
    let email_sender = Arc::new(email_sender);

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        moderation,
        redis,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        cache: None,
        invalidator: None,
        db_circuit_breaker: Arc::new(stellar_tipjar_backend::services::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(60))),
        lock_service: None,
    });

    (create_app(state), "mock_token".into())
}
