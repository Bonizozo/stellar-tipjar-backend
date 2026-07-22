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
use crate::errors::{AppError, StellarError};
use crate::models::pagination::PaginationParams;
use crate::models::tip::{
    RecordTipRequest, ReportMessageRequest, TipFilters, TipResponse, TipSortParams,
};
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
    headers: HeaderMap,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<RecordTipRequest>,
) -> Result<impl IntoResponse, AppError> {
    let client_ip = extract_client_ip(&headers);
    let validator = TipValidationService::new(ValidationRules::default());
    validator
        .validate_with_client(&state.db, &body, client_ip)
        .await?;

    // ── Pre-submission pipeline (issue #530) ──────────────────────────────

    // 1. Validate memo byte length before building the transaction.
    if let Some(ref memo) = body.memo {
        crate::services::stellar_service::StellarService::validate_memo(memo)?;
    }

    // 2. Check destination exists (errors with DestinationUnfunded if not).
    //    We validate the creator's registered wallet address.
    //    This is a best-effort check — if Horizon is down we proceed and let
    //    verify_transaction() surface the network error instead.
    if let Some(ref wallet) = body.tipper_wallet {
        // Only validate spendable balance when the sender wallet is provided.
        if let Err(e) = state
            .stellar
            .validate_spendable_balance(wallet, &body.amount)
            .await
        {
            // Surface balance / destination errors; swallow network errors so a
            // temporarily unreachable Horizon doesn't block all tips.
            match &e {
                AppError::Stellar(StellarError::InsufficientBalance { .. })
                | AppError::Stellar(StellarError::DestinationUnfunded { .. }) => {
                    return Err(e);
                }
                _ => {
                    tracing::warn!(error = %e, "Pre-flight balance check failed; proceeding to submission");
                }
            }
        }
    }

    // ── Stellar transaction verification ──────────────────────────────────
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

    let tip = tip_controller::record_tip_with_context(
        &state,
        body,
        tip_controller::TipRecordContext { ip: client_ip },
    )
    .await?;
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
    Ok((headers, StatusCode::OK, Json(serde_json::json!(response))).into_response())
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
