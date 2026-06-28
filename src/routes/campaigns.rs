use axum::{extract::{Path, State}, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::campaign_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::campaign::{CampaignContribution, CampaignResponse};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators/:username/campaigns", get(list_creator_campaigns))
        .route(
            "/campaigns/:campaign_id/contributions",
            get(get_campaign_contributions),
        )
}

async fn list_creator_campaigns(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let campaigns = campaign_controller::list_campaigns_for_creator(&state.db, &username).await?;
    Ok((StatusCode::OK, Json(campaigns)).into_response())
}

async fn get_campaign_contributions(
    State(state): State<Arc<AppState>>,
    Path(campaign_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let contributions =
        campaign_controller::get_campaign_contributions(&state.db, campaign_id).await?;
    Ok((StatusCode::OK, Json(contributions)).into_response())
}
