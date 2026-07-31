use crate::errors::app_error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub latest_indexed: u32,
    pub latest_network: u32,
    pub blocks_behind: u32,
    pub is_synced: bool,
}

pub struct SyncManager {
    pool: PgPool,
}

impl SyncManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn sync_from_genesis(&self) -> Result<(), AppError> {
        tracing::info!("Starting sync from genesis");
        Ok(())
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatus, AppError> {
        let latest_indexed = self.get_latest_indexed_ledger().await?;
        let latest_network = self.get_latest_network_ledger().await?;

        Ok(SyncStatus {
            latest_indexed,
            latest_network,
            blocks_behind: latest_network.saturating_sub(latest_indexed),
            is_synced: latest_network.saturating_sub(latest_indexed) < 10,
        })
    }

    async fn get_latest_indexed_ledger(&self) -> Result<u32, AppError> {
        let result: (Option<i32>,) =
            sqlx::query_as("SELECT MAX(ledger_sequence) FROM indexed_events")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::database_error(e.to_string()))?;

        Ok(result.0.unwrap_or(0) as u32)
    }

    async fn get_latest_network_ledger(&self) -> Result<u32, AppError> {
        Ok(1000)
    }
}
