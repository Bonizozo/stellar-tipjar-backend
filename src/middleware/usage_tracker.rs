use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use std::time::Instant;

use crate::db::connection::AppState;

/// Axum middleware that records method, path, status code, and duration for every request.
pub async fn track_api_usage(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(req).await;

    let status = response.status().as_u16() as i16;
    let duration_ms = start.elapsed().as_millis() as i32;
    let db = state.db.clone();

    tokio::spawn(async move {
        if let Err(e) = sqlx::query(
            "INSERT INTO api_usage_logs (method, path, status_code, duration_ms) VALUES ($1, $2, $3, $4)",
        )
        .bind(&method)
        .bind(&path)
        .bind(status)
        .bind(duration_ms)
        .execute(&db)
        .await
        {
            tracing::warn!(error = %e, "Failed to log API usage");
        }
    });

    response
}
