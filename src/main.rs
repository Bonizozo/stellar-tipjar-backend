use axum::Router;
use axum::{http::Method, Router};
use axum::{http::Method, Router};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tower_http::trace::TraceLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod analytics;
mod cache;
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
mod moderation;
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

    // Stick to the working tracing setup from your branch
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "stellar_tipjar_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let stellar_rpc_url = std::env::var("STELLAR_RPC_URL")
        .unwrap_or_else(|_| "https://soroban-testnet.stellar.org".to_string());
    let stellar_network =
        std::env::var("STELLAR_NETWORK").unwrap_or_else(|_| "testnet".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
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

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        redis,
        broadcast_tx,
        moderation,
    });

    // Start the real-time analytics pipeline as a background task.
    analytics::stream_processor::spawn(Arc::clone(&state));

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_origin(Any)
        .allow_headers(Any);

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
        .layer(axum::middleware::from_fn(
            middleware::deprecation::deprecation_notice,
        ));

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

    let gql_schema = graphql::schema::build_schema(Arc::clone(&state));

    let app = Router::new()
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route(
            "/graphql",
            axum::routing::post(graphql_handler).get(graphql_ws_handler),
        )
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(v1)
        .merge(v2)
        .layer(axum::Extension(gql_schema))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(
            middleware::tracing::trace_request,
        ))
        .layer(axum::middleware::from_fn(middleware::cache::cache_control))
        .layer(middleware::timeout::timeout_layer_from_env())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
