use axum::{response::IntoResponse, http::StatusCode, Json};
use std::process::Command;

pub async fn trigger_backup() -> impl IntoResponse {
    let output = Command::new("./scripts/backup.sh")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            (StatusCode::OK, Json(serde_json::json!({ "message": "Backup triggered successfully" })))
        }
        Ok(out) => {
            let error = String::from_utf8_lossy(&out.stderr);
            tracing::error!("Backup script failed: {}", error);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "Backup script execution failed" })))
        }
        Err(e) => {
            tracing::error!("Failed to execute backup script: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "Could not find or run backup script" })))
        }
    }
}