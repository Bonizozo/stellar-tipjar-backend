use axum::Router;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod analytics;
mod cache;
mod mocking;
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
mod gateway;
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

use crate::metrics::{metrics_handler, metrics_summary_handler};
use crate::middleware::metrics::track_metrics;
use db::connection::AppState;
use docs::ApiDoc;
use graphql::schema::{graphql_handler, graphql_ws_handler};
use service_mesh::discovery::ServiceRegistry;
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

    // --- Encryption Manager Initialization ---
    let encryption_manager = Arc::new(crate::crypto::encryption::EncryptionKeyManager::new().load().await?);
    crate::crypto::encryption::set_global_encryption_manager(Arc::clone(&encryption_manager))?;

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

    // Initialize read replicas from DATABASE_REPLICA_URL_1, _2, ... env vars.
    let replica_manager = {
        let mgr = db::replica::ReplicaManager::new(
            std::env::var("REPLICA_MAX_LAG_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50 * 1024 * 1024), // 50 MB default
        );
        let mut idx = 1u32;
        loop {
            let key = format!("DATABASE_REPLICA_URL_{}", idx);
            match std::env::var(&key) {
                Ok(url) => {
                    match db::connection::connect_with_retry(&url, 10, 2, Duration::from_secs(3), 3, 3, 30).await {
                        Ok(replica_pool) => {
                            tracing::info!(replica = idx, "Read replica connected");
                            mgr.add_replica(url, replica_pool).await;
                        }
                        Err(e) => tracing::warn!(replica = idx, error = %e, "Failed to connect replica"),
                    }
                    idx += 1;
                }
                Err(_) => break,
            }
        }
        let stats = mgr.stats().await;
        if stats.total > 0 {
            tracing::info!(total = stats.total, "Read replicas initialized");
            Some(Arc::new(mgr))
        } else {
            tracing::info!("No read replicas configured");
            None
        }
    };

    // Initialize multi-layer cache and invalidator
    let cache = Arc::new(cache::MultiLayerCache::with_defaults());
    let invalidator = Arc::new(cache::CacheInvalidator::new(Arc::clone(&cache), None));

    // Initialize sharding manager (#233)
    // Gracefully disabled when SHARD_COUNT=1 and no SHARD_n_DSN env vars are set.
    let sharding = db::sharding::init_sharding(&database_url).await;

    // Build distributed lock service before AppState so it can be stored directly.
    let lock_service = redis.as_ref().map(|conn| {
        Arc::new(services::distributed_lock::DistributedLockService::new(conn.clone()))
    });

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
        encryption: Arc::clone(&encryption_manager),
        replicas: replica_manager.clone(),
        lock_service: lock_service.clone(),
    });

    // Build CommandBus after state is constructed (avoids circular dependency).
    let command_bus = Arc::new(cqrs::CommandBus::new(
        Arc::clone(&state),
        Arc::clone(&event_store),
    ));

    // Start replica lag monitoring background task.
    if let Some(ref mgr) = replica_manager {
        let mgr = Arc::clone(mgr);
        let primary = pool.clone();
        tokio::spawn(async move {
            mgr.as_ref().clone().monitor_loop(primary, Duration::from_secs(10)).await;
        });
    }

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
    services::tx_pool_service::spawn(Arc::clone(&state));

    // Start background job processing system
    let (_job_queue, _job_scheduler) = jobs::start(Arc::clone(&state), jobs::JobConfig::default());

    // Start RabbitMQ queue system (optional — skipped when RABBITMQ_URL is unset).
    let queue_system = queue::try_start(Arc::clone(&state)).await;

    // Start cron scheduler (cleanup, weekly reports, cache warming, analytics)
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            match scheduler::SchedulerManager::new(pool.clone()).await {
                Ok(mgr) => {
                    if let Err(e) = mgr.start(pool).await {
                        tracing::error!("Scheduler failed to start: {e}");
                    }
                }
                Err(e) => tracing::error!("Failed to create scheduler: {e}"),
            }
        });
    }

    // Start Stellar transaction monitoring (#175)
    let monitor = services::monitoring_service::spawn(Arc::clone(&state));

    // Spawn distributed lock monitor (#267).
    if let Some(ref svc) = lock_service {
        services::distributed_lock::spawn_monitor(Arc::clone(svc));
    }

    // Service mesh: registry for health/canary endpoints (#245)
    let service_registry = Arc::new(ServiceRegistry::new());

    // CDN service — endpoint and TTL configurable via env vars.
    let cdn_endpoint = std::env::var("CDN_ENDPOINT")
        .unwrap_or_else(|_| "https://cdn.example.com".to_string());
    let cdn_ttl: u32 = std::env::var("CDN_CACHE_TTL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(86400);
    let cdn_service = {
        let svc = cdn::CdnService::new(cdn_endpoint, cdn_ttl);
        // Additional regions from CDN_REGION_n_NAME / CDN_REGION_n_ENDPOINT env vars.
        let mut extra_regions = Vec::new();
        let mut i = 1u32;
        loop {
            let name_key = format!("CDN_REGION_{}_NAME", i);
            let ep_key = format!("CDN_REGION_{}_ENDPOINT", i);
            match (std::env::var(&name_key), std::env::var(&ep_key)) {
                (Ok(name), Ok(endpoint)) => {
                    extra_regions.push(cdn::CdnRegion { name, endpoint });
                    i += 1;
                }
                _ => break,
            }
        }
        let svc = if !extra_regions.is_empty() {
            svc.with_regions(extra_regions)
        } else {
            svc
        };
        Arc::new(svc)
    };

    // Deprecation tracker — shared across v1 routes.
    let deprecation_tracker = Arc::new(middleware::deprecation::DeprecationTracker::new());

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

    // ── API Gateway layer (#232) ───────────────────────────────────────────────
    // Applied to all versioned API routes.  Provides:
    //   • Unified authentication (JWT + API key) with GatewayIdentity injection
    //   • Request transformation (body normalisation, header mutations)
    //   • Version negotiation with deprecation headers
    //   • Gateway-level metrics (latency header + structured log)
    //   • Request-ID propagation to response
    //   • Caller identity header (dev/staging only)
    let gateway_auth_layer = axum::middleware::from_fn_with_state(
        Arc::clone(&state),
        gateway::gateway_auth,
    );

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
                .merge(routes::deprecation::router())
                .merge(routes::tenants::router())
                .merge(
                    Router::new()
                        .merge(routes::auth::router())
                        .merge(routes::teams::router())
                        .merge(routes::tips::router())
                        .merge(routes::comments::router())
                        .merge(routes::creators::write_router())
                        .merge(routes::verification::router())
                        .merge(routes::goals::router())
                        .merge(routes::scheduled_tips::router())
                        .merge(routes::tx_pool::router())
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
                        .merge(routes::receipts::router())
                        .merge(routes::location::router())
                        .merge(routes::locks::router())
                        .layer(general_limiter_v1),
                )
                // Inject deprecation tracker for the /deprecation-status endpoint.
                // Extension layer is outermost so it runs first, making the tracker
                // available to the deprecation_notice middleware below.
                .layer(axum::middleware::from_fn(middleware::deprecation::deprecation_notice))
                .layer(axum::Extension(Arc::clone(&deprecation_tracker))),
        )
        // Gateway layers (innermost → outermost, applied bottom-up by Tower)
        .layer(axum::middleware::from_fn(gateway::inject_identity_header))
        .layer(axum::middleware::from_fn(gateway::gateway_metrics))
        .layer(axum::middleware::from_fn(gateway::propagate_request_id_to_response))
        .layer(axum::middleware::from_fn(gateway::transform_request))
        .layer(axum::middleware::from_fn(gateway::version_negotiation))
        .layer(gateway_auth_layer.clone());

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
            .merge(routes::tenants::router())
            .merge(
                Router::new()
                    .merge(routes::auth::router())
                    .merge(routes::teams::router())
                    .merge(routes::tips::router())
                    .merge(routes::creators::write_router())
                    .merge(routes::verification::router())
                    .merge(routes::goals::router())
                    .merge(routes::scheduled_tips::router())
                    .merge(routes::tx_pool::router())
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
                    .merge(routes::receipts::router())
                    .merge(routes::location::router())
                    .merge(routes::locks::router())
                    .layer(general_limiter_v2),
            ),
    )
    // Gateway layers
    .layer(axum::middleware::from_fn(gateway::inject_identity_header))
    .layer(axum::middleware::from_fn(gateway::gateway_metrics))
    .layer(axum::middleware::from_fn(gateway::propagate_request_id_to_response))
    .layer(axum::middleware::from_fn(gateway::transform_request))
    .layer(axum::middleware::from_fn(gateway::version_negotiation))
    .layer(gateway_auth_layer);

    let x_request_id = axum::http::HeaderName::from_static("x-request-id");

    let gql_schema = graphql::schema::build_schema(Arc::clone(&state));

    let app = Router::new()
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route(
            "/graphql",
            axum::routing::post(graphql_handler).get(graphql_ws_handler),
        )
        .route("/metrics", axum::routing::get(metrics_handler))
        .route("/metrics/summary", axum::routing::get(metrics_summary_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(routes::monitoring::router(Arc::clone(&state), Arc::clone(&monitor)))
        .merge(routes::mesh::router(Arc::clone(&state), Arc::clone(&service_registry)))
        .merge(routes::load_balancer::router(Arc::clone(&state), Arc::clone(&service_registry)))
        .merge(routes::cdn::router(Arc::clone(&cdn_service)))
        .merge(routes::profiling::router(Arc::clone(&state)))
        .merge(v1)
        .merge(v2)
        .layer(axum::Extension(gql_schema))
        // Inject CurrencyService for currency routes.
        .layer(axum::Extension(currency_svc))
        // Inject Redis connection into request extensions for distributed throttling.
        .layer(axum::Extension(state.redis.clone()))
        // Inject CommandBus for event-sourcing write operations.
        .layer(axum::Extension(command_bus))
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
        // Flush and shut down the OTel tracer so all buffered spans are
        // exported before the process exits.
        telemetry::shutdown_tracer();
        // Stop RabbitMQ consumer workers.
        if let Some(qs) = &queue_system {
            qs.shutdown();
        }
        tracing::info!("Graceful shutdown complete");
    })
    .await?;

    Ok(())
}
