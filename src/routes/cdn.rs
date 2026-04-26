use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use base64::Engine as _;
use serde::Deserialize;
use std::sync::Arc;

use crate::cdn::CdnService;

#[derive(Deserialize)]
pub struct UploadBody {
    pub file_name: String,
    pub content_type: String,
    /// Base64-encoded file data.
    pub data_base64: String,
}

/// POST /cdn/upload
pub async fn upload(
    State(cdn): State<Arc<CdnService>>,
    Json(body): Json<UploadBody>,
) -> impl IntoResponse {
    let data = match base64::engine::general_purpose::STANDARD.decode(&body.data_base64) {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid base64 data" })),
            )
                .into_response()
        }
    };

    match cdn.upload_file(body.file_name, body.content_type, data).await {
        Ok(resp) => (StatusCode::CREATED, Json(serde_json::json!(resp))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /cdn/cache/:file_id — invalidates CDN cache for a file across all regions.
pub async fn invalidate(
    State(cdn): State<Arc<CdnService>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    match cdn.invalidate_cache(&file_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /cdn/regions/:file_id — returns CDN URLs for a file across all regions.
pub async fn regions(
    State(cdn): State<Arc<CdnService>>,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    let urls = cdn.get_cdn_urls_all_regions(&file_id);
    Json(serde_json::json!({ "regions": urls }))
}

/// GET /cdn/metrics — returns CDN performance counters.
pub async fn metrics(State(cdn): State<Arc<CdnService>>) -> impl IntoResponse {
    Json(cdn.metrics_snapshot())
}

pub fn router(cdn: Arc<CdnService>) -> Router {
    Router::new()
        .route("/cdn/upload", post(upload))
        .route("/cdn/cache/:file_id", delete(invalidate))
        .route("/cdn/regions/:file_id", get(regions))
        .route("/cdn/metrics", get(metrics))
        .with_state(cdn)
}
