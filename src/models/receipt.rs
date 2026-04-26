use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Data needed to render a tip receipt PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptData {
    pub receipt_id: Uuid,
    pub tip_id: Uuid,
    pub transaction_hash: String,
    pub creator_username: String,
    pub amount_xlm: String,
    /// Optional tax rate as a decimal fraction (e.g. 0.10 for 10%)
    pub tax_rate: Option<f64>,
    /// Computed tax amount in XLM
    pub tax_amount: Option<String>,
    /// Total including tax
    pub total_amount: String,
    pub issued_at: DateTime<Utc>,
    pub network: String,
}

impl ReceiptData {
    pub fn new(
        tip_id: Uuid,
        transaction_hash: String,
        creator_username: String,
        amount_xlm: String,
        tax_rate: Option<f64>,
        network: String,
    ) -> Self {
        let amount: f64 = amount_xlm.parse().unwrap_or(0.0);
        let (tax_amount, total) = if let Some(rate) = tax_rate {
            let tax = amount * rate;
            let total = amount + tax;
            (
                Some(format!("{:.7}", tax)),
                format!("{:.7}", total),
            )
        } else {
            (None, amount_xlm.clone())
        };

        Self {
            receipt_id: Uuid::new_v4(),
            tip_id,
            transaction_hash,
            creator_username,
            amount_xlm,
            tax_rate,
            tax_amount,
            total_amount: total,
            issued_at: Utc::now(),
            network,
        }
    }
}
