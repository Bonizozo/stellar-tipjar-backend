use axum::{response::IntoResponse, routing::get, Json, Router};
use std::sync::Arc;

use crate::db::connection::AppState;

/// GET /api/v1/migration-guide — explains how to migrate from v1 to v2.
pub async fn migration_guide() -> impl IntoResponse {
    Json(serde_json::json!({
        "from": "v1",
        "to": "v2",
        "sunset_date": "2027-01-01",
        "changes": [
            {
                "endpoint": "GET /api/v1/creators/:username",
                "change": "Response now includes `email` and `created_at` fields in v2.",
                "v2_endpoint": "GET /api/v2/creators/:username"
            },
            {
                "endpoint": "GET /api/v1/creators/:username/tips",
                "change": "Response is now paginated with metadata. Tips include `message` field.",
                "v2_endpoint": "GET /api/v2/creators/:username/tips"
            },
            {
                "endpoint": "POST /api/v1/tips",
                "change": "Response now includes full tip details including `message` and `created_at`.",
                "v2_endpoint": "POST /api/v2/tips"
            },
            {
                "endpoint": "GET /api/v1/tips",
                "change": "New endpoint in v2 supporting filtering and sorting.",
                "v2_endpoint": "GET /api/v2/tips"
            }
        ],
        "docs": "https://docs.example.com/migration/v1-to-v2",
        "support": "support@example.com"
    }))
}

/// GET /api/v1/deprecation-status — returns usage stats for deprecated endpoints.
pub async fn deprecation_status(
    axum::extract::Extension(tracker): axum::extract::Extension<
        Arc<crate::middleware::deprecation::DeprecationTracker>,
    >,
) -> impl IntoResponse {
    let snapshot = tracker.snapshot().await;
    Json(serde_json::json!({
        "deprecated_since": "2026-01-01",
        "sunset_date": "2027-01-01",
        "endpoint_usage": snapshot,
    }))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/migration-guide", get(migration_guide))
        .route("/deprecation-status", get(deprecation_status))
}
