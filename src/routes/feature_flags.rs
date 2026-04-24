use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::controllers::feature_flag_controller;
use crate::db::connection::AppState;
use crate::middleware::admin_auth::require_admin;
use crate::models::feature_flag::{CreateFlagRequest, UpdateFlagRequest};

#[derive(Deserialize)]
struct EvalQuery {
    username: String,
}

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    let admin_routes = Router::new()
        .route("/admin/feature-flags", get(list_flags).post(create_flag))
        .route(
            "/admin/feature-flags/:name",
            get(get_flag).put(update_flag).delete(delete_flag),
        )
        .route_layer(middleware::from_fn_with_state(state, require_admin));

    let public_routes = Router::new()
        .route("/feature-flags/:name/evaluate", get(evaluate_flag));

    Router::new().merge(admin_routes).merge(public_routes)
}

async fn list_flags(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match feature_flag_controller::list_flags(&state.db).await {
        Ok(flags) => (StatusCode::OK, Json(serde_json::json!(flags))).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn get_flag(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match feature_flag_controller::get_flag(&state.db, &name).await {
        Ok(Some(flag)) => (StatusCode::OK, Json(serde_json::json!(flag))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Flag not found" })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn create_flag(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateFlagRequest>,
) -> impl IntoResponse {
    match feature_flag_controller::create_flag(&state.db, body).await {
        Ok(flag) => (StatusCode::CREATED, Json(serde_json::json!(flag))).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn update_flag(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<UpdateFlagRequest>,
) -> impl IntoResponse {
    match feature_flag_controller::update_flag(&state.db, &name, body).await {
        Ok(Some(flag)) => (StatusCode::OK, Json(serde_json::json!(flag))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Flag not found" })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn delete_flag(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match feature_flag_controller::delete_flag(&state.db, &name).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({ "message": "Flag deleted" })),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Flag not found" })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn evaluate_flag(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(q): Query<EvalQuery>,
) -> impl IntoResponse {
    match feature_flag_controller::evaluate(&state.db, &name, &q.username).await {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({ "flag": name, "enabled": enabled })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}
