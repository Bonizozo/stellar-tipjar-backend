use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use std::sync::Arc;

use crate::controllers::tip_controller;
use crate::db::connection::AppState;
use crate::errors::{AppError, StellarError};
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
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<RecordTipRequest>,
) -> Result<impl IntoResponse, AppError> {
    match state
        .stellar
        .verify_transaction(&body.transaction_hash)
        .await
    {
        Ok(false) => {
            return Err(AppError::Stellar(StellarError::TransactionNotFound {
                hash: body.transaction_hash.clone(),
            }));
        }
        Err(e) => return Err(e),
        Ok(true) => {}
    }

    let tip = tip_controller::record_tip(&state, body).await?;
    let response: TipResponse = tip.into();
    Ok((StatusCode::CREATED, Json(serde_json::json!(response))).into_response())
}
