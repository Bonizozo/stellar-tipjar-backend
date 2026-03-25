pub mod fixtures;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use std::sync::Arc;
use stellar_tipjar_backend::db::connection::AppState;
use stellar_tipjar_backend::services::stellar_service::StellarService;
use stellar_tipjar_backend::{cache, db, email, create_app};
use axum::Router;

pub async fn setup_test_db() -> PgPool {
    dotenvy::from_filename(".env.test").ok();
    dotenvy::dotenv().ok(); // Fallback to .env
    
    let database_url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set in .env.test or environment");
    
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
        .await
        .unwrap();
    
    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .unwrap();
    
    pool
}

pub async fn cleanup_test_db(pool: &PgPool) {
    sqlx::query("TRUNCATE creators, tips CASCADE")
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
    
    // Mock redis (or just let it fail/disable)
    let redis = None;
    
    // Initialize email system
    let (email_sender, _email_rx) = email::sender::EmailSender::new();
    let email_sender = Arc::new(email_sender);

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        redis,
        email: email_sender,
    });

    (create_app(state), "mock_token".into())
}
