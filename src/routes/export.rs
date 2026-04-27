use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::controllers::export_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::middleware::admin_auth::require_admin;

#[derive(Debug, Deserialize)]
pub struct ExportFormat {
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "json".to_string()
}

#[derive(Debug, Deserialize)]
pub struct BackupListQuery {
    #[serde(default = "default_backup_limit")]
    pub limit: i64,
}

fn default_backup_limit() -> i64 {
    20
}

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/export/creators", get(export_creators))
        .route("/export/tips", get(export_all_tips))
        .route("/export/creators/:username/tips", get(export_creator_tips))
        .route("/export/creators/:username/data", get(export_creator_data))
        .route("/backups", get(list_backups))
        .route("/backups/trigger", post(trigger_backup))
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
                response
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, "text/csv".parse().unwrap());
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

/// Export full creator data package (profile + tips + analytics) as JSON or CSV
async fn export_creator_data(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Query(params): Query<ExportFormat>,
) -> Response {
    match export_controller::get_creator_data_package(&state.db, &username).await {
        Ok(package) => match params.format.to_lowercase().as_str() {
            "csv" => {
                // CSV: export tips with analytics summary header
                let mut wtr = csv::Writer::from_writer(vec![]);
                wtr.write_record([
                    "id",
                    "amount",
                    "transaction_hash",
                    "message",
                    "created_at",
                ])
                .unwrap();
                for t in &package.tips {
                    wtr.write_record([
                        t.id.as_str(),
                        t.amount.as_str(),
                        t.transaction_hash.as_str(),
                        t.message.as_deref().unwrap_or(""),
                        t.created_at.as_str(),
                    ])
                    .unwrap();
                }
                let data = wtr.into_inner().unwrap_or_default();
                let filename = format!("{}_data.csv", username);
                let mut response = (StatusCode::OK, data).into_response();
                response
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, "text/csv".parse().unwrap());
                response.headers_mut().insert(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename)
                        .parse()
                        .unwrap(),
                );
                response
            }
            _ => (StatusCode::OK, Json(package)).into_response(),
        },
        Err(e) => {
            tracing::error!("Failed to export data for {}: {}", username, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to export creator data" })),
            )
                .into_response()
        }
    }
}

/// List recent backup records
async fn list_backups(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BackupListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let records = export_controller::list_backups(&state.db, params.limit).await?;
    Ok((StatusCode::OK, Json(records)))
}

/// Trigger a manual backup (records the attempt and runs backup script)
async fn trigger_backup(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Record backup attempt
    let _ = export_controller::record_backup(
        &state.db,
        "manual",
        "initiated",
        None,
        None,
        None,
    )
    .await;

    // Run backup script
    let output = std::process::Command::new("./scripts/backup.sh").output();

    match output {
        Ok(out) if out.status.success() => {
            // Parse the JSON output from the backup script
            let stdout = String::from_utf8_lossy(&out.stdout);
            let backup_info: serde_json::Value = match serde_json::from_str(&stdout.trim()) {
                Ok(json) => json,
                Err(_) => serde_json::json!({}),
            };

            let size = backup_info.get("size").and_then(|v| v.as_i64());
            let location = backup_info.get("file").and_then(|v| v.as_str()).map(|s| s.to_string());
            let checksum = backup_info.get("checksum").and_then(|v| v.as_str()).map(|s| s.to_string());

            let _ = export_controller::record_backup(
                &state.db,
                "manual",
                "completed",
                size,
                location.as_deref(),
                checksum.as_deref(),
            )
            .await;

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "completed",
                    "file": location,
                    "size_bytes": size,
                    "checksum": checksum
                })),
            )
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            tracing::error!("Backup script failed: {}", err);
            let _ = export_controller::record_backup(
                &state.db,
                "manual",
                "failed",
                None,
                None,
                None,
            )
            .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "failed", "error": err })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to run backup script: {}", e);
            let _ = export_controller::record_backup(
                &state.db,
                "manual",
                "failed",
                None,
                None,
                None,
            )
            .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "failed", "error": e.to_string() })),
            )
        }
    }
}

fn render_tips_csv(tips: &[crate::models::tip::Tip], filename: &str) -> Response {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "id",
        "creator_username",
        "amount",
        "transaction_hash",
        "created_at",
    ])
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
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "text/csv".parse().unwrap());
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", filename)
            .parse()
            .unwrap(),
    );
    response
}
