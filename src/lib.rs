pub mod admin;
pub mod analytics;
pub mod mocking;
pub mod cache;
pub mod chaos;
pub mod config;
pub mod controllers;
pub mod cqrs;
pub mod crypto;
pub mod currency;
pub mod db;
pub mod deployment;
pub mod docs;
pub mod email;
pub mod errors;
pub mod events;
pub mod gateway;
pub mod graphql;
pub mod indexer;
pub mod jobs;
pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod ml;
pub mod moderation;
pub mod models;
pub mod queue;
pub mod routes;
pub mod saga;
pub mod scheduler;
pub mod search;
pub mod security;
pub mod service_mesh;
pub mod services;
pub mod sharding;
pub mod shutdown;
pub mod telemetry;
pub mod tenancy;
pub mod upload;
pub mod validation;
pub mod webhooks;
pub mod webrtc;
pub mod ws;

use axum::{http::Method, Router};
use db::connection::AppState;
use docs::ApiDoc;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub fn create_app(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_origin(Any)
        .allow_headers(Any);

    // Build rate limiters. They handle their own background cleanup.
    let general_limiter = middleware::rate_limiter::general_limiter();
    let write_limiter = middleware::rate_limiter::write_limiter();

    // Write endpoints get a stricter per-IP limit.
    let write_routes = Router::new()
        .merge(routes::auth::router())
        .merge(routes::teams::router())
        .merge(routes::tips::router())
        .merge(routes::creators::write_router())
        .merge(routes::verification::router())
        .merge(routes::goals::router())
        .merge(routes::refunds::public_router())
        .layer(write_limiter);

    // Read endpoints use the general limit and intelligent response caching.
    let read_routes = Router::new()
        .merge(routes::creators::read_router())
        .merge(routes::deployment::router())
        .merge(routes::health::router())
        .merge(routes::leaderboard::router())
        .merge(routes::stats::router())
        .merge(routes::analytics::router())
        .merge(routes::receipts::router())
        .layer(general_limiter);

    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(routes::feature_flags::router(Arc::clone(&state)))
        .merge(routes::usage_analytics::router(Arc::clone(&state)))
        .merge(routes::refunds::admin_router(Arc::clone(&state)))
        .merge(write_routes)
        .merge(read_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::compression::compression_layer())
        .layer(middleware::timeout::timeout_layer_from_env())
        .layer(axum::middleware::from_fn(
            middleware::rate_limiter::whitelist_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            middleware::usage_tracker::track_api_usage,
        ))
        .with_state(state)
}
