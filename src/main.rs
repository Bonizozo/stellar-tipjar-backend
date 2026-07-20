use axum::Router;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use tracing_subscriber::util::SubscriberInitExt as _;

mod analytics;
mod cache;
mod config;
mod controllers;
mod cqrs;
mod db;
mod docs;
mod email;
mod errors;
mod events;
mod graphql;
mod logging;
mod metrics;
mod middleware;
mod models;
mod routes;
mod saga;
mod search;
mod security;
mod services;
mod shutdown;
mod telemetry;
mod validation;
mod webhooks;
mod ws;

use db::connection::AppState;
use docs::ApiDoc;
use services::stellar_service::StellarService;
use crate::metrics::metrics_handler;
use crate::middleware::metrics::track_metrics;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Initialise structured logging + optional OTEL tracing.
    // This must happen before AppConfig::from_env() so that validation errors
    // are formatted correctly.
    logging::init();

    // ── Fail-fast configuration loading ─────────────────────────────────────
    // Collects ALL validation errors before aborting — never panics on missing env.
    let cfg = config::AppConfig::from_env().map_err(|e| {
        // Print in human-readable form and propagate as a non-panic anyhow error.
        eprintln!("\n{e}\n");
        anyhow::anyhow!("Startup aborted: configuration is invalid")
    })?;
    let cfg = Arc::new(cfg);

    let pool = db::connection::connect(
        &cfg.database.url,
        20,
        5,
        Duration::from_secs(3),
        5,
    )
    .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let stellar = StellarService::new(
        cfg.stellar.rpc_url.clone(),
        cfg.stellar.network.clone(),
    );
    let performance = Arc::new(db::performance::PerformanceMonitor::new());
    let (broadcast_tx, _) = broadcast::channel(ws::CHANNEL_CAPACITY);

    let redis = cache::redis_client::connect(&cfg.redis.url).await;

    let (email_sender, email_rx) = email::sender::EmailSender::new(&cfg.smtp);
    let smtp_cfg = cfg.smtp.clone();
    tokio::spawn(email::sender::start_email_worker_with_config(smtp_cfg, email_rx));
    let _email_sender = Arc::new(email_sender);

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        redis,
        broadcast_tx,
        config: Arc::clone(&cfg),
    });

    analytics::stream_processor::spawn(Arc::clone(&state));

    let cors = middleware::cors::cors_layer(&cfg.cors);

    let general_limiter_v1 =
        middleware::rate_limiter::general_limiter(&cfg.rate_limit);
    let write_limiter_v1 =
        middleware::rate_limiter::write_limiter(&cfg.rate_limit);
    let general_limiter_v2 =
        middleware::rate_limiter::general_limiter(&cfg.rate_limit);
    let write_limiter_v2 =
        middleware::rate_limiter::write_limiter(&cfg.rate_limit);

    let v1 = Router::new()
        .nest(
            "/api/v1",
            Router::new()
                .merge(routes::admin::router(Arc::clone(&state)))
                .merge(
                    Router::new()
                        .merge(routes::tips::router())
                        .merge(routes::creators::write_router())
                        .layer(write_limiter_v1),
                )
                .merge(
                    Router::new()
                        .merge(routes::creators::read_router())
                        .merge(routes::health::router())
                        .layer(general_limiter_v1),
                ),
        )
        .layer(axum::middleware::from_fn(middleware::deprecation::deprecation_notice));

    let v2 = Router::new().nest(
        "/api/v2",
        Router::new()
            .merge(routes::admin::router(Arc::clone(&state)))
            .merge(
                Router::new()
                    .merge(routes::tips::router())
                    .merge(routes::creators::write_router())
                    .layer(write_limiter_v2),
            )
            .merge(
                Router::new()
                    .merge(routes::creators::read_router())
                    .merge(routes::health::router())
                    .layer(general_limiter_v2),
            ),
    );

    let x_request_id = axum::http::HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route("/metrics", axum::routing::get(metrics_handler))
        .merge(
            SwaggerUi::new("/swagger-ui")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .merge(v1)
        .merge(v2)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::tracing::trace_request))
        .layer(axum::middleware::from_fn(track_metrics))
        .layer(axum::middleware::from_fn(middleware::cache::cache_control))
        .layer(middleware::timeout::timeout_layer(cfg.request_timeout))
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            x_request_id.clone(),
            tower_http::request_id::MakeRequestUuid,
        ))
        .layer(tower_http::request_id::PropagateRequestIdLayer::new(
            x_request_id,
        ))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown::shutdown_signal())
    .await?;

    Ok(())
}
