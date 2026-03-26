use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::controllers::export_controller;
use crate::db::connection::AppState;
use crate::middleware::admin_auth::require_admin;

#[derive(Debug, Deserialize)]
pub struct ExportFormat {
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "json".to_string()
}

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/export/creators", get(export_creators))
        .route("/export/tips", get(export_all_tips))
        .route("/export/creators/:username/tips", get(export_creator_tips))
        .route_layer(middleware::from_fn_with_state(state, require_admin))
}

/// Export all creators as JSON or CSV
async fn export_creators(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportFormat>,
) -> Response {
    match export_controller::get_all_creators(&state.db).await {
        Ok(creators) => match params.format.to_lowercase().as_str() {
            "csv" => {
                let mut wtr = csv::Writer::from_writer(vec![]);
                wtr.write_record(["id", "username", "wallet_address", "created_at"])
                    .unwrap();
                for c in &creators {
                    wtr.write_record([
                        c.id.to_string(),
                        c.username.clone(),
                        c.wallet_address.clone(),
                        c.created_at.to_rfc3339(),
                    ])
                    .unwrap();
                }
                let data = wtr.into_inner().unwrap_or_default();
                let mut response = (StatusCode::OK, data).into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    "text/csv".parse().unwrap(),
                );
                response.headers_mut().insert(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"creators.csv\"".parse().unwrap(),
                );
                response
            }
            _ => (StatusCode::OK, Json(serde_json::json!(creators))).into_response(),
        },
        Err(e) => {
            tracing::error!("Failed to export creators: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to export creators" })),
            )
                .into_response()
        }
    }
}

/// Export all tips as JSON or CSV
async fn export_all_tips(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportFormat>,
) -> Response {
    match export_controller::get_all_tips(&state.db).await {
        Ok(tips) => match params.format.to_lowercase().as_str() {
            "csv" => render_tips_csv(&tips, "tips.csv"),
            _ => (StatusCode::OK, Json(serde_json::json!(tips))).into_response(),
        },
        Err(e) => {
            tracing::error!("Failed to export tips: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to export tips" })),
            )
                .into_response()
        }
    }
}

/// Export tips for a specific creator as JSON or CSV
async fn export_creator_tips(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(params): Query<ExportFormat>,
) -> Response {
    match export_controller::get_tips_for_creator(&state.db, &username).await {
        Ok(tips) => match params.format.to_lowercase().as_str() {
            "csv" => {
                let filename = format!("{}_tips.csv", username);
                render_tips_csv(&tips, &filename)
            }
            _ => (StatusCode::OK, Json(serde_json::json!(tips))).into_response(),
        },
        Err(e) => {
            tracing::error!("Failed to export tips for {}: {}", username, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to export tips" })),
            )
                .into_response()
        }
    }
}

fn render_tips_csv(tips: &[crate::models::tip::Tip], filename: &str) -> Response {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record(["id", "creator_username", "amount", "transaction_hash", "created_at"])
        .unwrap();
    for t in tips {
        wtr.write_record([
            t.id.to_string(),
            t.creator_username.clone(),
            t.amount.clone(),
            t.transaction_hash.clone(),
            t.created_at.to_rfc3339(),
        ])
        .unwrap();
    }
    let data = wtr.into_inner().unwrap_or_default();
    let mut response = (StatusCode::OK, data).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "text/csv".parse().unwrap(),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", filename).parse().unwrap(),
    );
    response
}
