use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::{AppError, AppResult, DatabaseError};
use crate::models::receipt::ReceiptData;
use crate::services::receipt_service;

/// Fetch tip by ID and build receipt data, then generate PDF bytes.
pub async fn generate_tip_receipt(
    state: &AppState,
    tip_id: Uuid,
    tax_rate: Option<f64>,
) -> AppResult<(ReceiptData, Vec<u8>)> {
    let tip = sqlx::query!(
        r#"SELECT id, creator_username, amount, transaction_hash FROM tips WHERE id = $1"#,
        tip_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        AppError::Database(DatabaseError::NotFound {
            entity: "tip",
            identifier: tip_id.to_string(),
        })
    })?;

    let network = std::env::var("STELLAR_NETWORK").unwrap_or_else(|_| "testnet".to_string());

    let receipt = ReceiptData::new(
        tip.id,
        tip.transaction_hash,
        tip.creator_username,
        tip.amount,
        tax_rate,
        network,
    );

    let pdf_bytes = receipt_service::generate_receipt_pdf(&receipt)?;
    Ok((receipt, pdf_bytes))
}
