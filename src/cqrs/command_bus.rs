use super::commands::{Command, CommandResult};
use crate::db::connection::AppState;
use crate::errors::AppResult;
use std::sync::Arc;

use super::handlers::{CommandHandler, RecordTipHandler, RegisterCreatorHandler};
use super::synchronizer::CqrsSynchronizer;
use crate::events::EventStore;

/// Executes write-side commands, persists to the write DB, and appends domain events.
pub struct CommandBus {
    handlers: Vec<Arc<dyn CommandHandler>>,
    synchronizer: Option<Arc<CqrsSynchronizer>>,
}

impl CommandBus {
    pub fn new(state: Arc<AppState>, events: Arc<EventStore>) -> Self {
        let handlers: Vec<Arc<dyn CommandHandler>> = vec![
            Arc::new(RegisterCreatorHandler::new(
                Arc::clone(&state),
                Arc::clone(&events),
            )),
            Arc::new(RecordTipHandler::new(
                Arc::clone(&state),
                Arc::clone(&events),
            )),
        ];
        Self {
            handlers,
            synchronizer: None,
        }
    }

    pub fn with_synchronizer(mut self, synchronizer: Arc<CqrsSynchronizer>) -> Self {
        self.synchronizer = Some(synchronizer);
        self
    }

    pub async fn execute(&self, cmd: Command) -> AppResult<CommandResult> {
        if let Some(handler) = self.handlers.iter().find(|handler| handler.handles(&cmd)) {
            let handled = handler.handle(cmd).await?;
            if let Some(result) = handled {
                if let Some(sync) = &self.synchronizer {
                    let _ = sync
                        .sync_with_retry(3, std::time::Duration::from_millis(100))
                        .await;
                }
                return Ok(result);
            }
        }

        Err(crate::errors::AppError::internal_with_message(
            "No command handler registered",
        ))
    }
}
