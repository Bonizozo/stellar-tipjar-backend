use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::controllers::receipt_controller;
use crate::db::connection::AppState;
use crate::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct ReceiptQuery {
    /// Tax rate as a percentage (e.g. 10 for 10%)
    pub tax_rate: Option<f64>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tips/:id/receipt", get(download_receipt))
        .route("/tips/:id/receipt/info", get(receipt_info))
}

async fn download_receipt(
    State(state): State<Arc<AppState>>,
    Path(tip_id): Path<Uuid>,
    Query(query): Query<ReceiptQuery>,
) -> Result<Response, AppError> {
    let tax_rate = query.tax_rate.map(|r| r / 100.0);
    let (receipt, pdf_bytes) =
        receipt_controller::generate_tip_receipt(&state, tip_id, tax_rate).await?;

    let filename = format!("receipt-{}.pdf", receipt.receipt_id);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .header(header::CONTENT_LENGTH, pdf_bytes.len())
        .body(Body::from(pdf_bytes))
        .map_err(|e| AppError::internal_with_message(format!("Response build error: {e}")))
}

async fn receipt_info(
    State(state): State<Arc<AppState>>,
    Path(tip_id): Path<Uuid>,
    Query(query): Query<ReceiptQuery>,
) -> Result<impl IntoResponse, AppError> {
    let tax_rate = query.tax_rate.map(|r| r / 100.0);
    let (receipt, _) =
        receipt_controller::generate_tip_receipt(&state, tip_id, tax_rate).await?;
    Ok((StatusCode::OK, Json(receipt)))
}
