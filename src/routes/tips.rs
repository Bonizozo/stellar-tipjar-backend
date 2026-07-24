use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::{net::IpAddr, sync::Arc};
use uuid::Uuid;

use crate::controllers::tip_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::pagination::PaginationParams;
use crate::models::tip::{
    RecordTipRequest, ReportMessageRequest, TipFilters, TipResponse, TipSortParams,
};
use crate::validation::ValidatedJson;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/tips", post(record_tip).get(list_tips))
        .route("/tips/:id/report", post(report_tip_message))
        // Opt-in Idempotency-Key handling (#342) for this router's mutating
        // routes. The middleware is a no-op for GET and for requests without
        // an Idempotency-Key header, so it's safe to attach to the whole
        // router rather than splitting POST out into its own sub-router.
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            crate::idempotency::middleware::idempotency_middleware,
        ))
}

/// Submit a tip for async on-chain verification.
///
/// The tip is stored immediately as `pending_verification`. The background
/// worker verifies the Stellar transaction and transitions the tip to
/// `confirmed` or `rejected`. Webhooks and leaderboard updates only fire
/// once the tip reaches `confirmed`.
#[utoipa::path(
    post,
    path = "/tips",
    tag = "tips",
    request_body = RecordTipRequest,
    responses(
        (status = 201, description = "Tip accepted for verification", body = TipResponse),
        (status = 400, description = "Invalid request body or validation failure"),
        (status = 404, description = "Creator not found"),
        (status = 409, description = "Duplicate transaction hash"),
        (status = 422, description = "Transaction not found or unsuccessful on Stellar network"),
        (status = 502, description = "Unable to reach Stellar network for verification"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn record_tip(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<RecordTipRequest>,
) -> Result<impl IntoResponse, AppError> {
    let tip = tip_controller::record_tip(&state, body).await?;
    let response: TipResponse = tip.into();
    Ok((StatusCode::CREATED, Json(response)))
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
    let uses_offset = params.uses_offset();
    let result = tip_controller::get_tips_paginated(&state, None, params, filters, sort).await?;
    let response = result.map(TipResponse::from);
    let mut headers = HeaderMap::new();
    if uses_offset {
        headers.insert("Deprecation", HeaderValue::from_static("true"));
        headers.insert(
            "X-Deprecation-Warning",
            HeaderValue::from_static("Offset pagination is deprecated; use signed keyset cursors."),
        );
    }
    Ok((headers, axum::http::StatusCode::OK, Json(serde_json::json!(response))).into_response())
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

pub(crate) fn extract_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        })
}
