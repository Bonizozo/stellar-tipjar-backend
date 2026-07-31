use crate::errors::app_error::AppError;
use crate::models::tip::Tip;
use sqlx::PgPool;

pub struct ReorgHandler {
    pool: PgPool,
}

impl ReorgHandler {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn handle_reorg(&self, reorg_depth: u32) -> Result<(), AppError> {
        tracing::warn!("Blockchain reorg detected, depth: {}", reorg_depth);

        let affected: Vec<Tip> = sqlx::query_as(
            "SELECT * FROM tips 
             WHERE indexed_at > NOW() - INTERVAL '1 hour'
             AND confirmations < $1",
        )
        .bind(reorg_depth as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::database_error(e.to_string()))?;

        for tip in affected {
            self.revalidate_transaction(&tip).await?;
        }

        Ok(())
    }

    async fn revalidate_transaction(&self, tip: &Tip) -> Result<(), AppError> {
        tracing::info!("Revalidating transaction: {}", tip.transaction_hash);
        Ok(())
    }
}
