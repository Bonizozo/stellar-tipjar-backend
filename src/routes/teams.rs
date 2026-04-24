use axum::{extract::{Path, State}, http::StatusCode, response::IntoResponse, routing::{delete, get, post, put}, Json, Router};
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::team_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::models::team::{CreateTeamRequest, TeamMemberRequest, UpdateMemberShareRequest};
use crate::validation::ValidatedJson;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/teams", post(create_team).get(list_teams))
        .route("/teams/:team_id", get(get_team))
        .route("/teams/:team_id/members", post(add_member))
        .route("/teams/:team_id/members/:username", put(update_member_share).delete(remove_member))
        .route("/teams/:team_id/splits", get(get_team_splits))
}

#[utoipa::path(
    post,
    path = "/teams",
    tag = "teams",
    request_body = CreateTeamRequest,
    responses(
        (status = 201, description = "Team created successfully"),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Conflict or duplicate member")
    )
)]
async fn create_team(
    State(state): State<Arc<AppState>>,
    ValidatedJson(body): ValidatedJson<CreateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    let team = team_controller::create_team(&state, body).await?;
    Ok((StatusCode::CREATED, Json(team)).into_response())
}

#[utoipa::path(
    get,
    path = "/teams",
    tag = "teams",
    responses(
        (status = 200, description = "List of teams")
    )
)]
async fn list_teams(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let teams = team_controller::list_teams(&state).await?;
    Ok((StatusCode::OK, Json(teams)).into_response())
}

#[utoipa::path(
    get,
    path = "/teams/{team_id}",
    tag = "teams",
    params(("team_id" = Uuid, Path, description = "Team ID")),
    responses(
        (status = 200, description = "Team details"),
        (status = 404, description = "Team not found")
    )
)]
async fn get_team(
    State(state): State<Arc<AppState>>,
    Path(team_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let team = team_controller::get_team(&state, team_id).await?;
    Ok((StatusCode::OK, Json(team)).into_response())
}

#[utoipa::path(
    post,
    path = "/teams/{team_id}/members",
    tag = "teams",
    params(("team_id" = Uuid, Path, description = "Team ID")),
    request_body = TeamMemberRequest,
    responses(
        (status = 201, description = "Member added"),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Conflict or duplicate member")
    )
)]
async fn add_member(
    State(state): State<Arc<AppState>>,
    Path(team_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<TeamMemberRequest>,
) -> Result<impl IntoResponse, AppError> {
    let member = team_controller::add_member(&state, team_id, body).await?;
    Ok((StatusCode::CREATED, Json(member)).into_response())
}

#[utoipa::path(
    put,
    path = "/teams/{team_id}/members/{username}",
    tag = "teams",
    params(
        ("team_id" = Uuid, Path, description = "Team ID"),
        ("username" = String, Path, description = "Member username")
    ),
    request_body = UpdateMemberShareRequest,
    responses(
        (status = 200, description = "Member share updated"),
        (status = 400, description = "Validation error")
    )
)]
async fn update_member_share(
    State(state): State<Arc<AppState>>,
    Path((team_id, username)): Path<(Uuid, String)>,
    ValidatedJson(body): ValidatedJson<UpdateMemberShareRequest>,
) -> Result<impl IntoResponse, AppError> {
    let member = team_controller::update_member_share(&state, team_id, username, body.share_percentage).await?;
    Ok((StatusCode::OK, Json(member)).into_response())
}

#[utoipa::path(
    delete,
    path = "/teams/{team_id}/members/{username}",
    tag = "teams",
    params(
        ("team_id" = Uuid, Path, description = "Team ID"),
        ("username" = String, Path, description = "Member username")
    ),
    responses(
        (status = 204, description = "Member removed"),
        (status = 404, description = "Member or team not found")
    )
)]
async fn remove_member(
    State(state): State<Arc<AppState>>,
    Path((team_id, username)): Path<(Uuid, String)>,
) -> Result<impl IntoResponse, AppError> {
    team_controller::remove_member(&state, team_id, username).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/teams/{team_id}/splits",
    tag = "teams",
    params(("team_id" = Uuid, Path, description = "Team ID")),
    responses(
        (status = 200, description = "Team split history")
    )
)]
async fn get_team_splits(
    State(state): State<Arc<AppState>>,
    Path(team_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let splits = team_controller::get_split_history(&state, team_id).await?;
    Ok((StatusCode::OK, Json(splits)).into_response())
}
