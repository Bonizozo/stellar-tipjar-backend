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

use crate::controllers::scheduled_tip_controller as ctrl;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::scheduled_tip::{
    CreateScheduledTipRequest, ScheduledTipResponse, UpdateScheduledTipRequest,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/scheduled-tips", post(create))
        .route("/scheduled-tips/:id", get(get_one).patch(update).delete(cancel))
        .route("/scheduled-tips", get(list))
}

#[derive(Deserialize)]
struct ListQuery {
    tipper_ref: String,
}

async fn create(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateScheduledTipRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.is_recurring && body.recurrence_rule.is_none() {
        return Err(AppError::Validation(
            crate::errors::ValidationError::InvalidRequest {
                message: "recurrence_rule is required when is_recurring is true".into(),
            },
        ));
    }

    let tip = ctrl::create(&state.db, body).await.map_err(AppError::from)?;
    Ok((StatusCode::CREATED, Json(ScheduledTipResponse::from(tip))))
}

async fn get_one(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let tip = ctrl::get(&state.db, id).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => AppError::unauthorized("Scheduled tip not found"),
        other => AppError::from(other),
    })?;
    Ok((StatusCode::OK, Json(ScheduledTipResponse::from(tip))))
}

async fn list(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let tips = ctrl::list_for_tipper(&state.db, &q.tipper_ref)
        .await
        .map_err(AppError::from)?;
    let response: Vec<ScheduledTipResponse> = tips.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(response)))
}

async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateScheduledTipRequest>,
) -> Result<impl IntoResponse, AppError> {
    let tip = ctrl::update(&state.db, id, body).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => AppError::unauthorized("Scheduled tip not found or not pending"),
        other => AppError::from(other),
    })?;
    Ok((StatusCode::OK, Json(ScheduledTipResponse::from(tip))))
}

async fn cancel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let cancelled = ctrl::cancel(&state.db, id).await.map_err(AppError::from)?;
    if cancelled {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(AppError::unauthorized("Scheduled tip not found or not pending"))
    }
}
