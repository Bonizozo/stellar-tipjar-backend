use crate::errors::app_error::AppError;
use crate::indexer::cursor::CursorManager;
use crate::indexer::processor::EventProcessor;
use crate::metrics::collectors::{
    BLOCKCHAIN_EVENTS_FAILED_TOTAL, BLOCKCHAIN_EVENTS_PROCESSED_TOTAL,
    BLOCKCHAIN_INDEXER_LAG_LEDGERS, BLOCKCHAIN_RETRY_ATTEMPTS_TOTAL,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const MAX_RETRY_DELAY_SECS: u64 = 60;
const BASE_RETRY_DELAY_SECS: u64 = 2;

/// A raw Stellar contract event received from the Horizon SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StellarEvent {
    pub id: String,
    pub event_type: String,
    pub transaction_hash: String,
    pub paging_token: String,
}

/// Parsed Horizon transaction record (subset of fields we care about).
#[derive(Debug, Deserialize)]
struct HorizonTransaction {
    id: String,
    paging_token: String,
    hash: String,
    successful: bool,
    #[serde(rename = "type")]
    tx_type: Option<String>,
}

/// Horizon SSE `_links` envelope — used to detect end-of-stream.
#[derive(Debug, Deserialize)]
struct HorizonRecord {
    #[serde(flatten)]
    tx: HorizonTransaction,
}

pub struct BlockchainListener {
    horizon_url: String,
    contract_id: String,
    cursor: Arc<RwLock<String>>,
    cursor_manager: Arc<CursorManager>,
    processor: Arc<EventProcessor>,
    http: reqwest::Client,
}

impl BlockchainListener {
    pub fn new(
        horizon_url: String,
        contract_id: String,
        cursor_manager: Arc<CursorManager>,
        processor: Arc<EventProcessor>,
    ) -> Self {
        Self {
            horizon_url,
            contract_id,
            cursor: Arc::new(RwLock::new("0".to_string())),
            cursor_manager,
            processor,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Entry point — restores cursor from Redis then streams indefinitely with retry.
    pub async fn start_listening(&self) -> Result<(), AppError> {
        if let Ok(Some(saved)) = self.cursor_manager.get_cursor().await {
            *self.cursor.write().await = saved;
        }

        tracing::info!(
            contract_id = %self.contract_id,
            "Blockchain listener starting"
        );

        let mut retry_count: u32 = 0;

        loop {
            let cursor = self.cursor.read().await.clone();
            match self.stream_events(&cursor).await {
                Ok(()) => {
                    retry_count = 0;
                }
                Err(e) => {
                    BLOCKCHAIN_RETRY_ATTEMPTS_TOTAL.inc();
                    retry_count += 1;
                    let delay = Self::backoff_delay(retry_count);
                    tracing::warn!(
                        error = %e,
                        retry = retry_count,
                        delay_secs = delay.as_secs(),
                        "Listener error — retrying"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Streams transactions from Horizon using SSE (`?cursor=…&order=asc`).
    async fn stream_events(&self, cursor: &str) -> Result<(), AppError> {
        let url = format!(
            "{}/transactions?cursor={}&order=asc&limit=200",
            self.horizon_url, cursor
        );

        let response = self
            .http
            .get(&url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| AppError::internal_with_message(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::internal_with_message(format!(
                "Horizon returned HTTP {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| AppError::internal_with_message(e.to_string()))?;

        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            // SSE data lines start with "data: "
            let data = if let Some(stripped) = line.strip_prefix("data: ") {
                stripped
            } else {
                continue;
            };

            if data == "\"hello\"" || data == "\"bye\"" {
                continue;
            }

            match serde_json::from_str::<HorizonTransaction>(data) {
                Ok(tx) => {
                    self.handle_transaction(tx).await?;
                }
                Err(e) => {
                    tracing::debug!(error = %e, raw = %data, "Skipping unparseable SSE frame");
                }
            }
        }

        Ok(())
    }

    /// Filters and dispatches a single transaction to the event processor.
    async fn handle_transaction(&self, tx: HorizonTransaction) -> Result<(), AppError> {
        if !tx.successful {
            return Ok(());
        }

        // Only process transactions relevant to our contract.
        if !self.is_relevant(&tx) {
            self.update_cursor(&tx.paging_token).await?;
            return Ok(());
        }

        let event_type = self.classify_event(&tx);
        let event = StellarEvent {
            id: tx.id.clone(),
            event_type: event_type.clone(),
            transaction_hash: tx.hash.clone(),
            paging_token: tx.paging_token.clone(),
        };

        let result = match event_type.as_str() {
            "tip" => self.processor.process_tip_event(&event).await,
            "withdraw" => self.processor.process_withdraw_event(&event).await,
            _ => Ok(()),
        };

        match result {
            Ok(()) => {
                BLOCKCHAIN_EVENTS_PROCESSED_TOTAL
                    .with_label_values(&[&event_type])
                    .inc();
                tracing::debug!(
                    tx_hash = %tx.hash,
                    event_type = %event_type,
                    "Event processed"
                );
            }
            Err(e) => {
                BLOCKCHAIN_EVENTS_FAILED_TOTAL
                    .with_label_values(&["processing_error"])
                    .inc();
                tracing::error!(error = %e, tx_hash = %tx.hash, "Event processing failed");
                return Err(e);
            }
        }

        self.update_cursor(&tx.paging_token).await?;
        Ok(())
    }

    /// Returns true if the transaction involves our contract.
    fn is_relevant(&self, tx: &HorizonTransaction) -> bool {
        // In production this would inspect the transaction's operations/memo
        // to confirm it targets `self.contract_id`. For now we accept all
        // successful transactions so the pipeline is exercised end-to-end.
        !self.contract_id.is_empty() && tx.successful
    }

    /// Classifies a transaction as "tip", "withdraw", or "unknown".
    fn classify_event(&self, tx: &HorizonTransaction) -> String {
        match tx.tx_type.as_deref() {
            Some("payment") => "tip".to_string(),
            Some("account_merge") => "withdraw".to_string(),
            _ => "unknown".to_string(),
        }
    }

    /// Persists the cursor to Redis and updates the in-memory value.
    pub async fn update_cursor(&self, paging_token: &str) -> Result<(), AppError> {
        *self.cursor.write().await = paging_token.to_string();
        self.cursor_manager.save_cursor(paging_token).await
    }

    /// Exponential backoff capped at `MAX_RETRY_DELAY_SECS`.
    fn backoff_delay(attempt: u32) -> Duration {
        let secs = (BASE_RETRY_DELAY_SECS * 2u64.pow(attempt.min(10))).min(MAX_RETRY_DELAY_SECS);
        Duration::from_secs(secs)
    }

    /// Updates the Prometheus lag gauge.
    pub fn set_indexer_lag(&self, lag: f64) {
        BLOCKCHAIN_INDEXER_LAG_LEDGERS.set(lag);
    }
}
