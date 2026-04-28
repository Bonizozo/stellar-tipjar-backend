use crate::anonymization::audit::AnonymizationAudit;
use crate::anonymization::masker::{Anonymizer, STANDARD_PII_FIELDS};
use crate::anonymization::detector::PiiDetector;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub struct AnonymizationState {
    pub anonymizer: Arc<Anonymizer>,
    pub detector: Arc<PiiDetector>,
    pub audit: Arc<AnonymizationAudit>,
}

/// POST /anonymization/detect
/// Detect PII in provided text or JSON
#[derive(Deserialize)]
struct DetectRequest {
    text: String,
}

async fn detect_pii(
    State(state): State<Arc<AnonymizationState>>,
    Json(body): Json<DetectRequest>,
) -> impl IntoResponse {
    let detections = state.detector.detect(&body.text);
    Json(json!({
        "contains_pii": !detections.is_empty(),
        "detections": detections,
    }))
}

/// POST /anonymization/mask
#[derive(Deserialize)]
struct MaskRequest {
    text: Option<String>,
    json: Option<Value>,
    pii_fields: Option<Vec<String>>,
}

async fn mask_data(
    State(state): State<Arc<AnonymizationState>>,
    Json(body): Json<MaskRequest>,
) -> impl IntoResponse {
    if let Some(text) = body.text {
        let masked = state.anonymizer.mask_text(&text);
        return Json(json!({ "masked": masked })).into_response();
    }

    if let Some(json_val) = body.json {
        let fields: Vec<&str> = body
            .pii_fields
            .as_ref()
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| STANDARD_PII_FIELDS.to_vec());
        let masked = state.anonymizer.anonymize_json(&json_val, &fields);
        return Json(json!({ "masked": masked })).into_response();
    }

    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": "provide 'text' or 'json'"})),
    )
        .into_response()
}

/// GET /anonymization/audit
#[derive(Deserialize)]
struct AuditQuery {
    entity_type: Option<String>,
    limit: Option<i64>,
}

async fn get_audit(
    State(state): State<Arc<AnonymizationState>>,
    Query(q): Query<AuditQuery>,
) -> impl IntoResponse {
    match state
        .audit
        .list(q.entity_type.as_deref(), q.limit.unwrap_or(50))
        .await
    {
        Ok(records) => Json(records).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to fetch anonymization audit");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response()
        }
    }
}

/// POST /anonymization/audit
#[derive(Deserialize)]
struct AuditRequest {
    entity_type: String,
    entity_id: String,
    fields: Vec<String>,
    reason: String,
    performed_by: Option<Uuid>,
}

async fn record_audit(
    State(state): State<Arc<AnonymizationState>>,
    Json(body): Json<AuditRequest>,
) -> impl IntoResponse {
    let fields: Vec<&str> = body.fields.iter().map(|s| s.as_str()).collect();
    match state
        .audit
        .record(
            &body.entity_type,
            &body.entity_id,
            &fields,
            &body.reason,
            body.performed_by,
        )
        .await
    {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to record anonymization audit");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response()
        }
    }
}

pub fn router(state: Arc<AnonymizationState>) -> Router {
    Router::new()
        .route("/anonymization/detect", post(detect_pii))
        .route("/anonymization/mask", post(mask_data))
        .route("/anonymization/audit", get(get_audit).post(record_audit))
        .with_state(state)
}
