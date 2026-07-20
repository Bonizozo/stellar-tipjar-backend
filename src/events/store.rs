use sqlx::PgPool;
use uuid::Uuid;
use crate::events::types::Event;

pub struct EventStore {
    pool: PgPool,
}

/// Row type returned by the INSERT … RETURNING query.
#[derive(sqlx::FromRow)]
struct SequenceRow {
    sequence_number: i64,
}

/// Row type returned by event SELECT queries.
#[derive(sqlx::FromRow)]
struct EventDataRow {
    event_data: serde_json::Value,
}

impl EventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a single event and return its sequence number.
    pub async fn append(&self, event: &Event) -> Result<i64, sqlx::Error> {
        let data = serde_json::to_value(event).map_err(|e| {
            tracing::error!(error = %e, "Failed to serialize event");
            // Map serde error to a database encode error so callers get a typed error.
            sqlx::Error::Encode(Box::new(e))
        })?;

        let row = sqlx::query_as::<_, SequenceRow>(
            r#"INSERT INTO events (aggregate_id, event_type, event_data)
               VALUES ($1, $2, $3)
               RETURNING sequence_number"#,
        )
        .bind(event.aggregate_id())
        .bind(event.event_type())
        .bind(data)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.sequence_number)
    }

    /// Load all events for a specific aggregate (e.g. a creator's UUID).
    pub async fn load(&self, aggregate_id: Uuid) -> Result<Vec<Event>, sqlx::Error> {
        let rows = sqlx::query_as::<_, EventDataRow>(
            r#"SELECT event_data FROM events
               WHERE aggregate_id = $1
               ORDER BY sequence_number"#,
        )
        .bind(aggregate_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.event_data).ok())
            .collect())
    }

    /// Replay all events from a given sequence number onward.
    pub async fn replay_from(&self, from_sequence: i64) -> Result<Vec<Event>, sqlx::Error> {
        let rows = sqlx::query_as::<_, EventDataRow>(
            r#"SELECT event_data FROM events
               WHERE sequence_number >= $1
               ORDER BY sequence_number"#,
        )
        .bind(from_sequence)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.event_data).ok())
            .collect())
    }
}
