pub mod cursor;
pub mod listener;
pub mod processor;
pub mod reorg_handler;
pub mod sync;

pub use cursor::CursorManager;
pub use listener::BlockchainListener;
pub use processor::EventProcessor;
pub use reorg_handler::ReorgHandler;
pub use sync::SyncManager;

use crate::db::connection::AppState;
use std::sync::Arc;

/// Spawn the blockchain event listener as a background Tokio task.
///
/// Requires Redis (for cursor persistence). When Redis is unavailable the
/// listener still runs but restarts from cursor "0" on each process restart.
pub fn spawn(state: Arc<AppState>) {
    let horizon_url = std::env::var("HORIZON_URL")
        .unwrap_or_else(|_| "https://horizon-testnet.stellar.org".to_string());
    let contract_id = std::env::var("STELLAR_CONTRACT_ID").unwrap_or_default();

    let processor = Arc::new(EventProcessor::new(state.db.clone()));

    tokio::spawn(async move {
        match &state.redis {
            Some(redis) => {
                let cursor_manager = Arc::new(CursorManager::new(redis.clone()));
                let listener = BlockchainListener::new(
                    horizon_url,
                    contract_id,
                    cursor_manager,
                    processor,
                );
                if let Err(e) = listener.start_listening().await {
                    tracing::error!(error = %e, "Blockchain listener exited with error");
                }
            }
            None => {
                tracing::warn!("Redis unavailable — blockchain listener not started");
            }
        }
    });
}
