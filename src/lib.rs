// ── Lint configuration ────────────────────────────────────────────────────
// Forbid panic-inducing patterns in production code.  Both lints are
// downgraded to `allow` inside `#[cfg(test)]` blocks (see below) so that
// test helpers can still use the ergonomic `.unwrap()` / `.expect()` idiom.
//
// To suppress a specific site that is truly infallible, add a comment
// explaining the invariant and use `.expect("invariant: …")` instead of
// `.unwrap()`.  The `expect_used` lint will fire on bare `.expect()` calls
// too; use `// #[allow(clippy::expect_used)]` at the call site when the
// invariant has been documented.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod analytics;
pub mod cache;
pub mod config;
pub mod controllers;
pub mod cqrs;
pub mod db;
pub mod docs;
pub mod email;
pub mod errors;
pub mod events;
pub mod graphql;
pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod saga;
pub mod search;
pub mod security;
pub mod services;
pub mod shutdown;
pub mod telemetry;
pub mod validation;
pub mod webhooks;
pub mod ws;

use axum::Router;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use docs::ApiDoc;
use db::connection::AppState;

pub fn create_app(state: Arc<AppState>) -> Router {
    let cfg = &state.config;

    let cors = middleware::cors::cors_layer(&cfg.cors);

    let general_limiter = middleware::rate_limiter::general_limiter(&cfg.rate_limit);
    let write_limiter = middleware::rate_limiter::write_limiter(&cfg.rate_limit);

    // Write endpoints get a stricter per-IP limit and JSON content-type enforcement.
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
        .merge(
            SwaggerUi::new("/swagger-ui")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .merge(write_routes)
        .merge(read_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::timeout::timeout_layer(cfg.request_timeout))
        .with_state(state)
}
