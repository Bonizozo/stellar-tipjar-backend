use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::{creator_controller, tip_controller};
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::creator::CreateCreatorRequest;
use crate::models::pagination::{PaginatedResponse, PaginationParams};
use crate::models::tip::{RecordTipRequest, TipFilters, TipResponse, TipSortParams};

/// Enriched creator shape — v2 includes timestamps and email.
#[derive(Serialize)]
pub struct CreatorResponseV2 {
    pub id: Uuid,
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators", post(create_creator))
        .route("/creators/:username", get(get_creator))
        .route("/creators/:username/tips", get(get_creator_tips))
        .route("/tips", post(record_tip).get(list_tips))
}

async fn create_creator(
    State(state): State<Arc<AppState>>,
    crate::validation::ValidatedJson(body): crate::validation::ValidatedJson<CreateCreatorRequest>,
) -> Result<impl IntoResponse, AppError> {
    let c = creator_controller::create_creator(&state, body).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatorResponseV2 {
            id: c.id,
            username: c.username,
            wallet_address: c.wallet_address,
            email: c.email,
            created_at: c.created_at,
        }),
    ))
}

async fn get_creator(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let c = creator_controller::get_creator_or_not_found(&state, &username).await?;
    Ok(Json(CreatorResponseV2 {
        id: c.id,
        username: c.username,
        wallet_address: c.wallet_address,
        email: c.email,
        created_at: c.created_at,
    }))
}

async fn get_creator_tips(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(params): Query<PaginationParams>,
    Query(filters): Query<TipFilters>,
    Query(sort): Query<TipSortParams>,
) -> Result<impl IntoResponse, AppError> {
    let result =
        tip_controller::get_tips_paginated(&state, Some(&username), params, filters, sort).await?;
    Ok(Json(result.map(TipResponse::from)))
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
    Ok((StatusCode::CREATED, Json(TipResponse::from(t))))
}

async fn list_tips(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
    Query(filters): Query<TipFilters>,
    Query(sort): Query<TipSortParams>,
) -> Result<impl IntoResponse, AppError> {
    let result = tip_controller::get_tips_paginated(&state, None, params, filters, sort).await?;
    Ok(Json(result.map(TipResponse::from)))
}
