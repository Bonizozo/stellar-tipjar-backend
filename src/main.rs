use axum::Router;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod analytics;
mod cache;
mod cdn;
mod chaos;
mod config;
mod controllers;
mod cqrs;
mod crypto;
mod db;
mod deployment;
mod docs;
mod email;
mod errors;
mod events;
mod graphql;
mod indexer;
mod jobs;
mod logging;
mod metrics;
mod middleware;
mod ml;
mod moderation;
mod models;
mod queue;
mod routes;
mod saga;
mod scheduler;
mod search;
mod security;
mod service_mesh;
mod services;
mod sharding;
mod shutdown;
mod telemetry;
mod tenancy;
mod upload;
mod validation;
mod webhooks;
mod webrtc;
mod ws;

use crate::metrics::metrics_handler;
use crate::middleware::metrics::track_metrics;
use db::connection::AppState;
use docs::ApiDoc;
use graphql::schema::{graphql_handler, graphql_ws_handler};
use services::stellar_service::StellarService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("DEBUG: Docker Hot-Reload is working!");
    dotenvy::dotenv().ok();

    logging::init();

    // --- Secrets Resolution ---
    // Loads from Vault (if VAULT_ADDR + VAULT_TOKEN are set) with env-var fallback.
    let secrets = config::secrets::SecretsManager::new().load().await?;
    let database_url = secrets.database_url;
    let stellar_rpc_url = secrets.stellar_rpc_url;
    let stellar_network = secrets.stellar_network;


    let pool = db::connection::connect_with_retry(
        &database_url,
        20,   // max_connections
        5,    // min_connections
        Duration::from_secs(3),
        5,    // max_retries
        5,    // circuit breaker threshold
        60,   // circuit breaker recovery secs
    )
    .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    // --- Services Initialization (Merged from Main) ---
    let stellar = StellarService::new(stellar_rpc_url, stellar_network);
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let (broadcast_tx, _) = broadcast::channel(ws::CHANNEL_CAPACITY);

    // Redis setup (Your fixed version)
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis = cache::redis_client::connect(&redis_url).await;

    // Email Worker (Added from Main)
    let (email_sender, email_rx) = email::sender::EmailSender::new();
    tokio::spawn(email::sender::start_email_worker(email_rx));
    let email_sender = Arc::new(email_sender);

    // Service Layer Orchestration (Added from Main)
    let tip_service = Arc::new(services::tip_service::TipService::new());
    let creator_service = Arc::new(services::creator_service::CreatorService::new());

    let moderation = Arc::new(moderation::ModerationService::new(pool.clone()));

    // Initialize multi-layer cache and invalidator
    let cache = Arc::new(cache::MultiLayerCache::with_defaults());
    let invalidator = Arc::new(cache::CacheInvalidator::new(Arc::clone(&cache), None));

    let state = Arc::new(AppState {
        db: pool.clone(),
        stellar,
        performance,
        redis,
        broadcast_tx,
        moderation,
        db_circuit_breaker: Arc::new(services::circuit_breaker::CircuitBreaker::new(
            5,
            std::time::Duration::from_secs(60),
        )),
        cache: Some(Arc::clone(&cache)),
        invalidator: Some(Arc::clone(&invalidator)),
    });

    // Start cache warming background task
    {
        let warm_source = Arc::new(cache::CreatorWarmSource { pool: pool.clone() });
        let warmer = Arc::new(cache::CacheWarmer::new(
            Arc::clone(&cache),
            warm_source,
            std::time::Duration::from_secs(300),
        ));
        tokio::spawn(async move {
            warmer.warm_on_schedule(std::time::Duration::from_secs(300), 100).await;
        });
    }

    // Start the real-time analytics pipeline as a background task.
    analytics::stream_processor::spawn(Arc::clone(&state));

    // Start scheduled tip processor
    services::scheduled_tip_service::spawn(Arc::clone(&state));

    // Start background job processing system
    let (_job_queue, _job_scheduler) = jobs::start(Arc::clone(&state), jobs::JobConfig::default());

    // Start Stellar transaction monitoring (#175)
    let monitor = services::monitoring_service::spawn(Arc::clone(&state));

    // --- Currency service ---
    let currency_svc = Arc::new(currency::CurrencyService::new());
    // Refresh exchange rates every hour in the background.
    currency::spawn_refresh_task(
        (*currency_svc).clone(),
        state.redis.clone(),
        Duration::from_secs(3600),
    );

    let cors = middleware::cors::cors_layer();

    // Build rate limiters (Your FIXED version - no tuples!)
    let general_limiter_v1 = middleware::rate_limiter::general_limiter();
    let write_limiter_v1 = middleware::rate_limiter::write_limiter();
    let general_limiter_v2 = middleware::rate_limiter::general_limiter();
    let write_limiter_v2 = middleware::rate_limiter::write_limiter();

    // Versioned API Routes (Merged from Main)
    let v1 = Router::new()
        .nest(
            "/api/v1",
            Router::new()
                .merge(routes::admin::router(Arc::clone(&state)))
                .merge(routes::api_keys::router(Arc::clone(&state)))
                .merge(routes::verification::admin_router(Arc::clone(&state)))
                .merge(routes::feature_flags::router(Arc::clone(&state)))
                .merge(routes::usage_analytics::router(Arc::clone(&state)))
                .merge(routes::refunds::admin_router(Arc::clone(&state)))
                .merge(routes::audit_logs::router(Arc::clone(&state)))
                .merge(routes::export::router(Arc::clone(&state)))
                .merge(
                    Router::new()
                        .merge(routes::auth::router())
                        .merge(routes::teams::router())
                        .merge(routes::tips::router())
                        .merge(routes::creators::write_router())
                        .merge(routes::verification::router())
                        .merge(routes::goals::router())
                        .merge(routes::scheduled_tips::router())
                        .merge(routes::v1::router())
                        .layer(write_limiter_v1),
                )
                .merge(
                    Router::new()
                        .merge(routes::creators::read_router())
                        .merge(routes::health::router())
                        .merge(routes::notifications::router())
                        .merge(routes::leaderboard::router())
                        .merge(routes::stats::router())
                        .merge(routes::analytics::router())
                        .layer(general_limiter_v1),
                ),
        )
        .layer(axum::middleware::from_fn(
            middleware::version::version_headers,
        ));

    let v2 = Router::new().nest(
        "/api/v2",
        Router::new()
            .merge(routes::admin::router(Arc::clone(&state)))
            .merge(routes::api_keys::router(Arc::clone(&state)))
            .merge(routes::verification::admin_router(Arc::clone(&state)))
            .merge(routes::feature_flags::router(Arc::clone(&state)))
            .merge(routes::usage_analytics::router(Arc::clone(&state)))
            .merge(routes::refunds::admin_router(Arc::clone(&state)))
            .merge(routes::audit_logs::router(Arc::clone(&state)))
            .merge(routes::export::router(Arc::clone(&state)))
            .merge(
                Router::new()
                    .merge(routes::auth::router())
                    .merge(routes::teams::router())
                    .merge(routes::tips::router())
                    .merge(routes::creators::write_router())
                    .merge(routes::verification::router())
                    .merge(routes::goals::router())
                    .merge(routes::scheduled_tips::router())
                    .merge(routes::v2::router())
                    .layer(write_limiter_v2),
            )
            .merge(
                Router::new()
                    .merge(routes::creators::read_router())
                    .merge(routes::health::router())
                    .merge(routes::notifications::router())
                    .merge(routes::leaderboard::router())
                    .merge(routes::stats::router())
                    .merge(routes::analytics::router())
                    .layer(general_limiter_v2),
            ),
    )
    .layer(axum::middleware::from_fn(
        middleware::version::version_headers,
    ));

    let x_request_id = axum::http::HeaderName::from_static("x-request-id");

    let gql_schema = graphql::schema::build_schema(Arc::clone(&state));

    let app = Router::new()
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route(
            "/graphql",
            axum::routing::post(graphql_handler).get(graphql_ws_handler),
        )
        .route("/metrics", axum::routing::get(metrics_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(routes::monitoring::router(Arc::clone(&state), Arc::clone(&monitor)))
        .merge(routes::profiling::router(Arc::clone(&state)))
        .merge(v1)
        .merge(v2)
        .layer(axum::Extension(gql_schema))
        // Inject CurrencyService for currency routes.
        .layer(axum::Extension(currency_svc))
        // Inject Redis connection into request extensions for distributed throttling.
        .layer(axum::Extension(state.redis.clone()))
        .layer(cors)
        .layer(axum::middleware::map_response(middleware::cors::security_headers))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(
            middleware::tracing::trace_request,
        ))
        .layer(axum::middleware::from_fn(
            middleware::request_id::propagate_request_id,
        ))
        .layer(axum::middleware::from_fn(track_metrics))
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            x_request_id.clone(),
            tower_http::request_id::MakeRequestUuid,
        ))
        .layer(tower_http::request_id::PropagateRequestIdLayer::new(
            x_request_id,
        ))
        .layer(axum::middleware::from_fn(middleware::cache::cache_control))
        .layer(middleware::timeout::timeout_layer_from_env())
        // Redis distributed throttle (shared across instances, fail-open when Redis unavailable).
        .layer(axum::middleware::from_fn(
            middleware::rate_limiter::redis_throttle_middleware,
        ))
        .layer(axum::middleware::from_fn(
            middleware::rate_limiter::whitelist_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            middleware::usage_tracker::track_api_usage,
        ))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    // Graceful shutdown (#177): complete in-flight requests, then stop.
    let shutdown_timeout = shutdown::shutdown_timeout();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown::shutdown_signal().await;
        tracing::info!(
            timeout_secs = shutdown_timeout.as_secs(),
            "Waiting for in-flight requests to complete…"
        );
        tokio::time::sleep(shutdown_timeout).await;
        monitor.stop().await;
        tracing::info!("Graceful shutdown complete");
    })
    .await?;

    Ok(())
}
