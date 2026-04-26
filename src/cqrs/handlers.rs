use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::controllers::{creator_controller, tip_controller};
use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::events::{Event, EventStore};
use crate::models::creator::CreateCreatorRequest;
use crate::models::tip::RecordTipRequest;

use super::commands::{Command, CommandResult};

#[async_trait]
pub trait CommandHandler: Send + Sync {
    fn handles(&self, cmd: &Command) -> bool;
    async fn handle(&self, cmd: Command) -> AppResult<Option<CommandResult>>;
}

pub struct RegisterCreatorHandler {
    state: Arc<AppState>,
    events: Arc<EventStore>,
}

impl RegisterCreatorHandler {
    pub fn new(state: Arc<AppState>, events: Arc<EventStore>) -> Self {
        Self { state, events }
    }
}

#[async_trait]
impl CommandHandler for RegisterCreatorHandler {
    fn handles(&self, cmd: &Command) -> bool {
        matches!(cmd, Command::RegisterCreator { .. })
    }

    async fn handle(&self, cmd: Command) -> AppResult<Option<CommandResult>> {
        let Command::RegisterCreator {
            username,
            wallet_address,
            email,
        } = cmd
        else {
            return Ok(None);
        };

        let creator = creator_controller::create_creator(
            &self.state,
            CreateCreatorRequest {
                username,
                wallet_address,
                email,
            },
        )
        .await?;

        let event = Event::CreatorRegistered {
            id: creator.id,
            username: creator.username.clone(),
            wallet_address: creator.wallet_address.clone(),
            timestamp: Utc::now(),
        };
        let _ = self.events.append(&event).await;

        Ok(Some(CommandResult::CreatorRegistered { id: creator.id }))
    }
}

pub struct RecordTipHandler {
    state: Arc<AppState>,
    events: Arc<EventStore>,
}

impl RecordTipHandler {
    pub fn new(state: Arc<AppState>, events: Arc<EventStore>) -> Self {
        Self { state, events }
    }
}

#[async_trait]
impl CommandHandler for RecordTipHandler {
    fn handles(&self, cmd: &Command) -> bool {
        matches!(cmd, Command::RecordTip { .. })
    }

    async fn handle(&self, cmd: Command) -> AppResult<Option<CommandResult>> {
        let Command::RecordTip {
            creator_username,
            amount,
            transaction_hash,
        } = cmd
        else {
            return Ok(None);
        };

        let tip = tip_controller::record_tip(
            &self.state,
            RecordTipRequest {
                username: creator_username,
                amount,
                transaction_hash,
                message: None,
            },
        )
        .await?;

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

        Ok(Some(CommandResult::TipRecorded { id: tip.id }))
    }
}
