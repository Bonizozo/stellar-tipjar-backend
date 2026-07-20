pub mod fixtures;

use axum::Router;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use std::time::Duration;

use stellar_tipjar_backend::config::{
    AppConfig, CorsConfig, DatabaseConfig, JwtConfig, PaginationConfig, RateLimitConfig,
    RedisConfig, SmtpConfig, StellarConfig, TipValidationConfig,
};
use stellar_tipjar_backend::db::connection::AppState;
use stellar_tipjar_backend::db::performance::PerformanceMonitor;
use stellar_tipjar_backend::services::stellar_service::StellarService;
use stellar_tipjar_backend::create_app;

/// Build a minimal `AppConfig` suitable for tests — no env reads.
pub fn test_app_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        database: DatabaseConfig {
            url: std::env::var("TEST_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .unwrap_or_else(|_| "postgres://localhost/tipjar_test".into()),
        },
        redis: RedisConfig {
            url: "redis://127.0.0.1:6379".into(),
        },
        jwt: JwtConfig {
            secret: "test-jwt-secret-that-is-long-enough-and-has-entropy!@#$%".into(),
        },
        smtp: SmtpConfig {
            host: "localhost".into(),
            port: 1025,
            user: None,
            pass: None,
            from: "no-reply@test.com".into(),
        },
        cors: CorsConfig {
            allowed_origins: vec![],
            max_age_secs: 3600,
        },
        rate_limit: RateLimitConfig {
            general_per_second: 100,
            general_burst_size: 200,
            write_per_second: 50,
            write_burst_size: 100,
            whitelist: vec![],
        },
        stellar: StellarConfig {
            rpc_url: "https://soroban-testnet.stellar.org".into(),
            network: "testnet".into(),
        },
        pagination: PaginationConfig {
            max_offset: 10_000,
            cursor_secret: "test-cursor-secret-that-is-long-enough-for-hmac!".into(),
        },
        tip_validation: TipValidationConfig {
            min_amount: rust_decimal::Decimal::new(1, 2), // 0.01
            max_amount: rust_decimal::Decimal::new(10_000, 0),
            rate_limit_per_minute: 100,
        },
        webhook_secret: None,
        port: 8000,
        request_timeout: Duration::from_secs(30),
    })
}

pub async fn setup_test_db() -> PgPool {
    dotenvy::from_filename(".env.test").ok();
    dotenvy::dotenv().ok();

    let database_url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
        .await
        .expect("connect to test DB");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");

    pool
}

pub async fn cleanup_test_db(pool: &PgPool) {
    sqlx::query("TRUNCATE creators, tips CASCADE")
        .execute(pool)
        .await
        .expect("truncate test tables");
}

pub async fn create_test_app(pool: PgPool) -> (Router, String) {
    let cfg = test_app_config();
    let stellar = StellarService::new(
        cfg.stellar.rpc_url.clone(),
        cfg.stellar.network.clone(),
    );
    let performance = Arc::new(PerformanceMonitor::new());

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        redis: None,
        broadcast_tx: tokio::sync::broadcast::channel(16).0,
        config: Arc::clone(&cfg),
    });

    (create_app(state), "mock_token".into())
}
