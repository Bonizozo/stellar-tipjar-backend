use crate::errors::app_error::AppError;
use crate::indexer::listener::StellarEvent;
use sqlx::PgPool;

pub struct EventProcessor {
    pool: PgPool,
}

impl EventProcessor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn process_tip_event(&self, event: &StellarEvent) -> Result<(), AppError> {
        if self.is_processed(&event.id).await? {
            return Ok(());
        }
        tracing::info!(tx_hash = %event.transaction_hash, "Processing tip event");
        self.persist_event(event, "tip").await
    }

    pub async fn process_withdraw_event(&self, event: &StellarEvent) -> Result<(), AppError> {
        if self.is_processed(&event.id).await? {
            return Ok(());
        }
        tracing::info!(tx_hash = %event.transaction_hash, "Processing withdraw event");
        self.persist_event(event, "withdraw").await
    }

    /// Persists the full event record to `indexed_events`.
    async fn persist_event(&self, event: &StellarEvent, event_type: &str) -> Result<(), AppError> {
        // Extract ledger sequence from the paging_token (format: "<ledger>-<tx_index>").
        let ledger_sequence: i32 = event
            .paging_token
            .split('-')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        sqlx::query(
            "INSERT INTO indexed_events
                (event_id, event_type, transaction_hash, ledger_sequence, processed_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(&event.id)
        .bind(event_type)
        .bind(&event.transaction_hash)
        .bind(ledger_sequence)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(())
    }

    async fn is_processed(&self, event_id: &str) -> Result<bool, AppError> {
        let result: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM indexed_events WHERE event_id = $1")
                .bind(event_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(result.0 > 0)
    }
}
