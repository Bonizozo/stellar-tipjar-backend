use ax_id::HeaderName;
use axum::{Router, http::{Method, HeaderName as HttpHeaderName}};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use tokio::sync::broadcast;

mod cache;
mod controllers;
mod db;
mod docs;
mod metrics;
mod middleware;
mod models;
mod routes;
mod search;
mod services;
mod shutdown;
mod ws;           // Added from Main
mod webhooks;     // Added from Main
mod email;        // Added from Main (Ensure this module exists in your tree)

use db::connection::AppState;
use docs::ApiDoc;
use services::stellar_service::StellarService;
use crate::metrics::metrics_handler;
use crate::middleware::metrics::track_metrics;

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
    let stellar_network = std::env::var("STELLAR_NETWORK")
        .unwrap_or_else(|_| "testnet".to_string());

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
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis = cache::redis_client::connect(&redis_url).await;

    // Email Worker (Added from Main)
    let (email_sender, email_rx) = email::sender::EmailSender::new();
    tokio::spawn(email::sender::start_email_worker(email_rx));
    let email_sender = Arc::new(email_sender);

    // Service Layer Orchestration (Added from Main)
    let tip_service = Arc::new(services::tip_service::TipService::new());
    let creator_service = Arc::new(services::creator_service::CreatorService::new());

    let state = Arc::new(AppState {
        db: pool,
        stellar,
        performance,
        redis,
        tip_service,      // Must be added to AppState struct in next step
        creator_service,  // Must be added to AppState struct in next step
        email_sender,     // Must be added to AppState struct in next step
        broadcast_tx,     // Must be added to AppState struct in next step
    });

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

    let app = Router::new()
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route("/metrics", axum::routing::get(metrics_handler))
        .merge(SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(v1)
        .merge(v2)
        .layer(axum::middleware::from_fn(track_metrics)) // Your metrics middleware
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::timeout::timeout_layer_from_env())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;

    Ok(())
}