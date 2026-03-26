use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use std::sync::Arc;

use crate::controllers::tip_controller;
use crate::db::connection::AppState;
use crate::models::tip::{RecordTipRequest, TipResponse};

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/tips", post(record_tip))
}

/// Record a new tip (verifies transaction on the Stellar network first)
#[utoipa::path(
    post,
    path = "/tips",
    tag = "tips",
    request_body = RecordTipRequest,
    responses(
        (status = 201, description = "Tip recorded successfully", body = TipResponse),
        (status = 422, description = "Transaction not found or unsuccessful on Stellar network"),
        (status = 502, description = "Unable to reach Stellar network for verification"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn record_tip(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RecordTipRequest>,
) -> impl IntoResponse {
    match state
        .stellar
        .verify_transaction(&body.transaction_hash)
        .await
    {
        Ok(false) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": "Transaction not found or unsuccessful on the Stellar network" })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(
                "Failed to verify transaction {}: {}",
                body.transaction_hash,
                e
            );
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "Unable to verify transaction on the Stellar network" })),
            )
                .into_response();
        }
        Ok(true) => {}
    }

    match tip_controller::record_tip(&state, body).await {
        Ok(tip) => {
            let response: TipResponse = tip.into();
            (StatusCode::CREATED, Json(serde_json::json!(response))).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to record tip: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to record tip" })),
            )
                .into_response()
        }
    }
}
