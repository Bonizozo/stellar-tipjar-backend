//! API Documentation Portal — serves Redoc UI and SDK quickstart guide.

use axum::{
    http::header,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use std::sync::Arc;

use crate::db::connection::AppState;

const PORTAL_HTML: &str = include_str!("../../static/docs/portal.html");
const SDK_HTML: &str = include_str!("../../static/docs/sdk.html");

/// Serves the interactive Redoc documentation portal at `/docs`.
pub async fn portal_handler() -> impl IntoResponse {
    Html(PORTAL_HTML)
}

/// Serves the SDK quickstart guide at `/docs/sdk`.
pub async fn sdk_guide_handler() -> impl IntoResponse {
    Html(SDK_HTML)
}

/// Serves the raw OpenAPI JSON spec at `/docs/openapi.json`.
pub async fn openapi_json_handler() -> Response {
    use crate::docs::ApiDoc;
    use utoipa::OpenApi;

    let spec = ApiDoc::openapi().to_json().unwrap_or_default();
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        spec,
    )
        .into_response()
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/docs", get(portal_handler))
        .route("/docs/sdk", get(sdk_guide_handler))
        .route("/docs/openapi.json", get(openapi_json_handler))
}
