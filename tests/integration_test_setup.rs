// tests/integration_test_setup.rs
// Utility to build the Axum app for integration tests using the test configuration.

use std::sync::Arc;
use std::time::Duration;
use axum::{Router, routing::get};
use dotenvy::dotenv;
use crate::metrics::{metrics_handler, metrics_summary_handler};
use crate::docs::ApiDoc;
use utoipa_swagger_ui::SwaggerUi;
use crate::middleware::metrics::track_metrics;
use crate::middleware::cors::cors_layer;
use tower_http::request_id::{SetRequestIdLayer, MakeRequestUuid, PropagateRequestIdLayer};
use tower_http::trace::TraceLayer;
use crate::services::distributed_lock::DistributedLockService;
use crate::db::connection::{connect_with_retry, AppState};
use crate::routes;
use crate::middleware;
use crate::services;
use crate::gateway;
use crate::cdn;
use crate::currency;
use crate::analytics;
use crate::cache;
use crate::collaboration;
use crate::anonymization;
use crate::queue;
use crate::scheduler;
use crate::jobs;
use crate::monitoring;
use crate::telemetry;

pub async fn build_app() -> Router {
    // Load test env variables
    dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = connect_with_retry(
        &database_url,
        5, // max connections
        1, // min connections
        Duration::from_secs(1),
        2, // max retries
        2, // circuit breaker threshold
        30, // circuit breaker recovery secs
    )
    .await
    .expect("Failed to connect to DB");

    // Run migrations (assumes migrations folder present)
    sqlx::migrate!("./migrations").run(&pool).await.expect("Migrations failed");

    // Minimal set of services required for the tested routes
    let stellar = services::stellar_service::StellarService::new(
        std::env::var("STELLAR_RPC_URL").unwrap_or_default(),
        std::env::var("STELLAR_NETWORK").unwrap_or_default(),
    );
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let redis = None; // not needed for current tests
    let broadcast_tx = tokio::sync::broadcast::channel(16).0;
    let moderation = Arc::new(moderation::ModerationService::new(pool.clone()));
    let state = Arc::new(AppState {
        db: pool.clone(),
        stellar,
        performance,
        redis,
        broadcast_tx,
        moderation,
        db_circuit_breaker: Arc::new(services::circuit_breaker::CircuitBreaker::new(5, Duration::from_secs(60))),
        cache: None,
        invalidator: None,
        encryption: Arc::new(crate::crypto::encryption::EncryptionKeyManager::new().load().await.unwrap()),
        replicas: None,
        lock_service: None,
    });

    // Build router similar to main but only required routes for tests
    let cors = cors_layer();
    let v1 = Router::new()
        .nest(
            "/api/v1",
            Router::new()
                .merge(routes::creators::write_router())
                .merge(routes::creators::read_router())
                .merge(routes::tips::router())
                .layer(middleware::rate_limiter::write_limiter()),
        );

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/metrics/summary", get(metrics_summary_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(v1)
        .layer(axum::Extension(gateway::gateway_auth_layer()))
        .layer(cors)
        .layer(axum::middleware::from_fn(track_metrics))
        .layer(SetRequestIdLayer::new(MakeRequestUuid, PropagateRequestIdLayer::new()))
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    app
}
