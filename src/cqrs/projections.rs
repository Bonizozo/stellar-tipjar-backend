use crate::errors::AppResult;
use crate::events::{Event, EventStore};
use sqlx::PgPool;
use std::sync::Arc;

/// Keeps a denormalised `creator_read_model` table in sync with domain events.
/// Call `sync_event` after every successful command to maintain consistency.
pub struct CqrsProjection {
    read_db: PgPool,
    events: Arc<EventStore>,
}

#[derive(Debug, Clone)]
pub struct ProjectionSyncReport {
    pub from_sequence: i64,
    pub to_sequence: i64,
    pub processed_events: usize,
}

impl CqrsProjection {
    pub fn new(read_db: PgPool, events: Arc<EventStore>) -> Self {
        Self { read_db, events }
    }

    /// Apply a single event to the read model.
    pub async fn sync_event(&self, event: &Event) -> AppResult<()> {
        match event {
            Event::CreatorRegistered {
                id,
                username,
                wallet_address,
                timestamp,
            } => {
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
        sqlx::query("TRUNCATE creator_read_model")
            .execute(&self.read_db)
            .await?;
        let events = self.events.replay_from(0).await?;
        for event in &events {
            self.sync_event(event).await?;
        }
        Ok(())
    }

    /// Incrementally apply events from a sequence number.
    /// Used by the CQRS synchronizer to provide eventual consistency.
    pub async fn sync_from_sequence(&self, from_sequence: i64) -> AppResult<ProjectionSyncReport> {
        let events = self.events.replay_from(from_sequence).await?;
        let mut processed = 0usize;
        for event in &events {
            self.sync_event(event).await?;
            processed += 1;
        }

        let to_sequence = if processed == 0 {
            from_sequence - 1
        } else {
            from_sequence + processed as i64 - 1
        };

        Ok(ProjectionSyncReport {
            from_sequence,
            to_sequence,
            processed_events: processed,
        })
    }

    /// Read-optimized view query for command/query separation.
    pub async fn get_creator_summary_by_username(
        &self,
        username: &str,
    ) -> AppResult<Option<CreatorSummaryView>> {
        let row = sqlx::query_as::<_, CreatorSummaryView>(
            "SELECT id, username, wallet_address, tip_count, registered_at
             FROM creator_read_model
             WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.read_db)
        .await?;
        Ok(row)
    }
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct CreatorSummaryView {
    pub id: uuid::Uuid,
    pub username: String,
    pub wallet_address: String,
    pub tip_count: i64,
    pub registered_at: chrono::DateTime<chrono::Utc>,
}
