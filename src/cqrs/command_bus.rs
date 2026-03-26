use std::sync::Arc;
use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::models::creator::CreateCreatorRequest;
use crate::models::tip::RecordTipRequest;
use crate::controllers::{creator_controller, tip_controller};
use crate::events::{Event, EventStore};
use chrono::Utc;
use super::commands::{Command, CommandResult};

/// Executes write-side commands, persists to the write DB, and appends domain events.
pub struct CommandBus {
    state: Arc<AppState>,
    events: Arc<EventStore>,
}

impl CommandBus {
    pub fn new(state: Arc<AppState>, events: Arc<EventStore>) -> Self {
        Self { state, events }
    }

    pub async fn execute(&self, cmd: Command) -> AppResult<CommandResult> {
        match cmd {
            Command::RegisterCreator { username, wallet_address, email } => {
                let creator = creator_controller::create_creator(
                    &self.state,
                    CreateCreatorRequest { username, wallet_address, email },
                )
                .await?;

                let event = Event::CreatorRegistered {
                    id: creator.id,
                    username: creator.username.clone(),
                    wallet_address: creator.wallet_address.clone(),
                    timestamp: Utc::now(),
                };
                let _ = self.events.append(&event).await;

                Ok(CommandResult::CreatorRegistered { id: creator.id })
            }

            Command::RecordTip { creator_username, amount, transaction_hash } => {
                let tip = tip_controller::record_tip(
                    &self.state,
                    RecordTipRequest { username: creator_username, amount, transaction_hash },
                )
                .await?;

                // Resolve creator_id for the event (best-effort; skip on miss).
                if let Ok(Some(creator)) =
                    creator_controller::get_creator_by_username(&self.state, &tip.creator_username).await
                {
                    let event = Event::TipReceived {
                        id: tip.id,
                        creator_id: creator.id,
                        amount: tip.amount.clone(),
                        transaction_hash: tip.transaction_hash.clone(),
                        timestamp: Utc::now(),
                    };
                    let _ = self.events.append(&event).await;
                }

                Ok(CommandResult::TipRecorded { id: tip.id })
            }
        }
    }
}
