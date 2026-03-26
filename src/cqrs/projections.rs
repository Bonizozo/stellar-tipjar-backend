use crate::events::{Event, EventStore};
use crate::errors::AppResult;
use sqlx::PgPool;
use std::sync::Arc;

/// Keeps a denormalised `creator_read_model` table in sync with domain events.
/// Call `sync_event` after every successful command to maintain consistency.
pub struct CqrsProjection {
    read_db: PgPool,
    events: Arc<EventStore>,
}

impl CqrsProjection {
    pub fn new(read_db: PgPool, events: Arc<EventStore>) -> Self {
        Self { read_db, events }
    }

    /// Apply a single event to the read model.
    pub async fn sync_event(&self, event: &Event) -> AppResult<()> {
        match event {
            Event::CreatorRegistered { id, username, wallet_address, timestamp } => {
                sqlx::query(
                    "INSERT INTO creator_read_model (id, username, wallet_address, tip_count, registered_at) \
                     VALUES ($1, $2, $3, 0, $4) \
                     ON CONFLICT (id) DO NOTHING",
                )
                .bind(id)
                .bind(username)
                .bind(wallet_address)
                .bind(timestamp)
                .execute(&self.read_db)
                .await?;
            }

            Event::TipReceived { creator_id, .. } => {
                sqlx::query(
                    "UPDATE creator_read_model SET tip_count = tip_count + 1 WHERE id = $1",
                )
                .bind(creator_id)
                .execute(&self.read_db)
                .await?;
            }
        }
        Ok(())
    }

    /// Full rebuild: replay all events from sequence 0 and reapply them.
    pub async fn rebuild(&self) -> AppResult<()> {
        sqlx::query("TRUNCATE creator_read_model").execute(&self.read_db).await?;
        let events = self.events.replay_from(0).await?;
        for event in &events {
            self.sync_event(event).await?;
        }
        Ok(())
    }
}
