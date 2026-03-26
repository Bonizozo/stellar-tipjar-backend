pub mod cache;
pub mod controllers;
pub mod db;
pub mod docs;
pub mod email;
pub mod errors;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod search;
pub mod services;
pub mod shutdown;
pub mod validation;
pub mod webhooks;
pub mod ws;

use axum::{Router, http::Method};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use docs::ApiDoc;
use db::connection::AppState;

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
        .merge(routes::tips::router())
        .merge(routes::creators::write_router())
        .layer(write_limiter);

    // Read endpoints use the general limit.
    let read_routes = Router::new()
        .merge(routes::creators::read_router())
        .merge(routes::health::router())
        .layer(general_limiter);

    Router::new()
        .merge(SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(write_routes)
        .merge(read_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::compression::compression_layer())
        .layer(middleware::timeout::timeout_layer_from_env())
        .with_state(state)
}
