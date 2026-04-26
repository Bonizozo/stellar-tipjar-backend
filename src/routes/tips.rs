use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::tip_controller;
use crate::db::connection::AppState;
use crate::errors::{AppError, StellarError};
use crate::models::pagination::PaginationParams;
use crate::models::tip::{RecordTipRequest, ReportMessageRequest, TipFilters, TipResponse, TipSortParams};
use crate::services::validation_service::{TipValidationService, ValidationRules};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tips", post(record_tip).get(list_tips))
        .route("/tips/:id/report", post(report_tip_message))
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
    let validator = TipValidationService::new(ValidationRules::default());
    validator.validate(&state.db, &body).await?;

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

/// List all tips with pagination, filtering, and sorting
#[utoipa::path(
    get,
    path = "/tips",
    tag = "tips",
    params(PaginationParams, TipFilters, TipSortParams),
    responses(
        (status = 200, description = "Paginated list of tips"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_tips(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
    Query(filters): Query<TipFilters>,
    Query(sort): Query<TipSortParams>,
) -> Result<impl IntoResponse, AppError> {
    let result = tip_controller::get_tips_paginated(&state, None, params, filters, sort).await?;
    let response = result.map(TipResponse::from);
    Ok((StatusCode::OK, Json(serde_json::json!(response))).into_response())
}

/// Report a tip message for moderation review
#[utoipa::path(
    post,
    path = "/tips/{id}/report",
    tag = "tips",
    params(("id" = Uuid, Path, description = "Tip ID")),
    request_body = ReportMessageRequest,
    responses(
        (status = 204, description = "Report submitted"),
        (status = 404, description = "Tip not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn report_tip_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<ReportMessageRequest>,
) -> Result<impl IntoResponse, AppError> {
    tip_controller::report_tip_message(&state, id, body).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
