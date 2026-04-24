use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::subscription_controller as ctrl;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::subscription::{
    AddBenefitRequest, BenefitResponse, CreateSubscriptionRequest, CreateTierRequest,
    PaymentResponse, RenewSubscriptionRequest, SubscriptionResponse, TierResponse,
    UpdateTierRequest,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Tier management (creator-facing)
        .route("/creators/:username/tiers", post(create_tier).get(list_tiers))
        .route("/tiers/:id", get(get_tier).patch(update_tier).delete(delete_tier))
        // Benefits
        .route("/tiers/:id/benefits", post(add_benefit).get(list_benefits))
        .route("/tiers/:tier_id/benefits/:benefit_id", delete(remove_benefit))
        // Subscriptions (supporter-facing)
        .route("/subscriptions", post(subscribe).get(list_subscriptions))
        .route("/subscriptions/:id", get(get_subscription))
        .route("/subscriptions/:id/cancel", post(cancel_subscription))
        .route("/subscriptions/:id/renew", post(renew_subscription))
        .route("/subscriptions/:id/payments", get(list_payments))
        // Creator view of their subscribers
        .route("/creators/:username/subscriptions", get(list_creator_subscriptions))
}

// ── Tier handlers ─────────────────────────────────────────────────────────────

async fn create_tier(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<CreateTierRequest>,
) -> Result<impl IntoResponse, AppError> {
    let (tier, benefits) = ctrl::create_tier(&state.db, &username, body)
        .await
        .map_err(AppError::from)?;

    let response = build_tier_response(tier, benefits);
    Ok((StatusCode::CREATED, Json(response)))
}

async fn get_tier(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let tier = ctrl::get_tier(&state.db, id).await.map_err(not_found_or)?;
    let benefits = ctrl::list_benefits(&state.db, id).await.map_err(AppError::from)?;
    Ok((StatusCode::OK, Json(build_tier_response(tier, benefits))))
}

async fn list_tiers(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let tiers = ctrl::list_tiers(&state.db, &username)
        .await
        .map_err(AppError::from)?;

    let mut responses = Vec::with_capacity(tiers.len());
    for tier in tiers {
        let id = tier.id;
        let benefits = ctrl::list_benefits(&state.db, id).await.map_err(AppError::from)?;
        responses.push(build_tier_response(tier, benefits));
    }
    Ok((StatusCode::OK, Json(responses)))
}

async fn update_tier(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTierRequest>,
) -> Result<impl IntoResponse, AppError> {
    let tier = ctrl::update_tier(&state.db, id, body)
        .await
        .map_err(not_found_or)?;
    let benefits = ctrl::list_benefits(&state.db, id).await.map_err(AppError::from)?;
    Ok((StatusCode::OK, Json(build_tier_response(tier, benefits))))
}

async fn delete_tier(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = ctrl::delete_tier(&state.db, id).await.map_err(AppError::from)?;
    if deleted {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(AppError::unauthorized("Tier not found"))
    }
}

// ── Benefit handlers ──────────────────────────────────────────────────────────

async fn add_benefit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddBenefitRequest>,
) -> Result<impl IntoResponse, AppError> {
    let benefit = ctrl::add_benefit(&state.db, id, body)
        .await
        .map_err(AppError::from)?;
    Ok((StatusCode::CREATED, Json(BenefitResponse::from(benefit))))
}

async fn list_benefits(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let benefits = ctrl::list_benefits(&state.db, id).await.map_err(AppError::from)?;
    let response: Vec<BenefitResponse> = benefits.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(response)))
}

async fn remove_benefit(
    State(state): State<Arc<AppState>>,
    Path((_tier_id, benefit_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let removed = ctrl::remove_benefit(&state.db, benefit_id)
        .await
        .map_err(AppError::from)?;
    if removed {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(AppError::unauthorized("Benefit not found"))
    }
}

// ── Subscription handlers ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SubscriberQuery {
    subscriber_ref: String,
}

async fn subscribe(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSubscriptionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let sub = ctrl::subscribe(&state.db, body).await.map_err(AppError::from)?;
    Ok((StatusCode::CREATED, Json(SubscriptionResponse::from(sub))))
}

async fn get_subscription(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let sub = ctrl::get_subscription(&state.db, id)
        .await
        .map_err(not_found_or)?;
    Ok((StatusCode::OK, Json(SubscriptionResponse::from(sub))))
}

async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SubscriberQuery>,
) -> Result<impl IntoResponse, AppError> {
    let subs = ctrl::list_subscriptions_for_subscriber(&state.db, &q.subscriber_ref)
        .await
        .map_err(AppError::from)?;
    let response: Vec<SubscriptionResponse> = subs.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(response)))
}

async fn list_creator_subscriptions(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let subs = ctrl::list_subscriptions_for_creator(&state.db, &username)
        .await
        .map_err(AppError::from)?;
    let response: Vec<SubscriptionResponse> = subs.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(response)))
}

async fn cancel_subscription(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let cancelled = ctrl::cancel_subscription(&state.db, id)
        .await
        .map_err(AppError::from)?;
    if cancelled {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(AppError::unauthorized("Subscription not found or not active"))
    }
}

async fn renew_subscription(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    body: Option<Json<RenewSubscriptionRequest>>,
) -> Result<impl IntoResponse, AppError> {
    let tx_hash = body.and_then(|b| b.transaction_hash.clone());
    let sub = ctrl::renew_subscription(&state.db, id, tx_hash)
        .await
        .map_err(not_found_or)?;
    Ok((StatusCode::OK, Json(SubscriptionResponse::from(sub))))
}

async fn list_payments(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let payments = ctrl::list_payments(&state.db, id).await.map_err(AppError::from)?;
    let response: Vec<PaymentResponse> = payments.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(response)))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_tier_response(
    tier: crate::models::subscription::SubscriptionTier,
    benefits: Vec<crate::models::subscription::TierBenefit>,
) -> TierResponse {
    TierResponse {
        id: tier.id,
        creator_username: tier.creator_username,
        name: tier.name,
        description: tier.description,
        price_xlm: tier.price_xlm,
        is_active: tier.is_active,
        position: tier.position,
        benefits: benefits.into_iter().map(BenefitResponse::from).collect(),
        created_at: tier.created_at,
    }
}

fn not_found_or(e: sqlx::Error) -> AppError {
    match e {
        sqlx::Error::RowNotFound => AppError::unauthorized("Resource not found"),
        other => AppError::from(other),
    }
}
