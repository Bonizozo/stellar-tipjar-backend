use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::goal_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::goal::{CreateGoalRequest, TipGoalResponse};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/creators/:username/goals", post(create_goal).get(list_goals))
        .route("/creators/:username/goals/:goal_id", get(get_goal).delete(cancel_goal))
}

async fn create_goal(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(body): Json<CreateGoalRequest>,
) -> Result<impl IntoResponse, AppError> {
    let goal = goal_controller::create_goal(&state.db, &username, body).await?;
    let resp: TipGoalResponse = goal.into();
    Ok((StatusCode::CREATED, Json(serde_json::json!(resp))).into_response())
}

async fn list_goals(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let goals = goal_controller::list_goals(&state.db, &username).await?;
    let resp: Vec<TipGoalResponse> = goals.into_iter().map(Into::into).collect();
    Ok((StatusCode::OK, Json(serde_json::json!(resp))).into_response())
}

async fn get_goal(
    State(state): State<Arc<AppState>>,
    Path((_username, goal_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let goal = goal_controller::get_goal(&state.db, goal_id).await?;
    let resp: TipGoalResponse = goal.into();
    Ok((StatusCode::OK, Json(serde_json::json!(resp))).into_response())
}

async fn cancel_goal(
    State(state): State<Arc<AppState>>,
    Path((username, goal_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let goal = goal_controller::cancel_goal(&state.db, goal_id, &username).await?;
    let resp: TipGoalResponse = goal.into();
    Ok((StatusCode::OK, Json(serde_json::json!(resp))).into_response())
}
