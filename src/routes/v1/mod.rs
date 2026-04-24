use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::{creator_controller, tip_controller};
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::tip::{RecordTipRequest, TipFilters, TipSortParams};
use crate::models::pagination::PaginationParams;

/// Slim creator shape — v1 omits timestamps and optional fields.
#[derive(Serialize)]
pub struct CreatorResponseV1 {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
}

/// Slim tip shape — v1 omits message.
#[derive(Serialize)]
pub struct TipResponseV1 {
    pub id: Uuid,
    pub creator_username: String,
    pub amount: String,
    pub transaction_hash: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators", post(create_creator))
        .route("/creators/:username", get(get_creator))
        .route("/creators/:username/tips", get(get_creator_tips))
        .route("/tips", post(record_tip))
}

async fn create_creator(
    State(state): State<Arc<AppState>>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<
        crate::models::creator::CreateCreatorRequest,
    >,
) -> Result<impl IntoResponse, AppError> {
    let c = creator_controller::create_creator(&state, body).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatorResponseV1 {
            id: c.id,
            username: c.username,
            wallet_address: c.wallet_address,
        }),
    ))
}

async fn get_creator(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let c = creator_controller::get_creator_or_not_found(&state, &username).await?;
    Ok(Json(CreatorResponseV1 {
        id: c.id,
        username: c.username,
        wallet_address: c.wallet_address,
    }))
}

async fn get_creator_tips(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    use axum::extract::Query;
    let params = PaginationParams { page: 1, limit: 20 };
    let result = tip_controller::get_tips_paginated(
        &state,
        Some(&username),
        params,
        TipFilters::default(),
        TipSortParams::default(),
    )
    .await?;
    let tips: Vec<TipResponseV1> = result
        .data
        .into_iter()
        .map(|t| TipResponseV1 {
            id: t.id,
            creator_username: t.creator_username,
            amount: t.amount,
            transaction_hash: t.transaction_hash,
        })
        .collect();
    Ok(Json(tips))
}

async fn record_tip(
    State(state): State<Arc<AppState>>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<RecordTipRequest>,
) -> Result<impl IntoResponse, AppError> {
    use crate::errors::StellarError;
    match state.stellar.verify_transaction(&body.transaction_hash).await {
        Ok(false) => {
            return Err(AppError::Stellar(StellarError::TransactionNotFound {
                hash: body.transaction_hash.clone(),
            }))
        }
        Err(e) => return Err(e),
        Ok(true) => {}
    }
    let t = tip_controller::record_tip(&state, body).await?;
    Ok((
        StatusCode::CREATED,
        Json(TipResponseV1 {
            id: t.id,
            creator_username: t.creator_username,
            amount: t.amount,
            transaction_hash: t.transaction_hash,
        }),
    ))
}
