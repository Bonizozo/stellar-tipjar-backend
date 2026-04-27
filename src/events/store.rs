use crate::events::projections::CreatorProjection;
use crate::events::types::Event;
use sqlx::PgPool;
use uuid::Uuid;

/// How many events to accumulate before writing a snapshot.
const SNAPSHOT_THRESHOLD: i64 = 50;

pub struct EventStore {
    pool: PgPool,
}

impl EventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a single event and return its sequence number.
    /// Automatically writes a snapshot every `SNAPSHOT_THRESHOLD` events.
    pub async fn append(&self, event: &Event) -> Result<i64, sqlx::Error> {
        let data = serde_json::to_value(event).expect("Event must be serializable");
        let row = sqlx::query!(
            r#"INSERT INTO events (aggregate_id, event_type, event_data, version)
               VALUES ($1, $2, $3, $4)
               RETURNING sequence_number"#,
            event.aggregate_id(),
            event.event_type(),
            data,
            event.version(),
        )
        .fetch_one(&self.pool)
        .await?;

        let seq = row.sequence_number;

        // Write a snapshot every N events for this aggregate.
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM events WHERE aggregate_id = $1",
        )
        .bind(event.aggregate_id())
        .fetch_one(&self.pool)
        .await?;

        if count.0 % SNAPSHOT_THRESHOLD == 0 {
            let _ = self.write_snapshot(event.aggregate_id(), seq).await;
        }

        Ok(seq)
    }

    /// Load all events for a specific aggregate ordered by sequence.
    pub async fn load(&self, aggregate_id: Uuid) -> Result<Vec<Event>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT event_data FROM events
               WHERE aggregate_id = $1
               ORDER BY sequence_number"#,
            aggregate_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.event_data).ok())
            .collect())
    }

    /// Load events for an aggregate starting from a given sequence number.
    pub async fn load_from(
        &self,
        aggregate_id: Uuid,
        from_sequence: i64,
    ) -> Result<Vec<Event>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT event_data FROM events
               WHERE aggregate_id = $1 AND sequence_number >= $2
               ORDER BY sequence_number"#,
            aggregate_id,
            from_sequence,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.event_data).ok())
            .collect())
    }

    /// Load events for an aggregate up to and including a sequence number.
    pub async fn load_up_to(
        &self,
        aggregate_id: Uuid,
        up_to_sequence: i64,
    ) -> Result<Vec<Event>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT event_data FROM events
               WHERE aggregate_id = $1 AND sequence_number <= $2
               ORDER BY sequence_number"#,
            aggregate_id,
            up_to_sequence,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.event_data).ok())
            .collect())
    }

    /// Replay all events from a given global sequence number onward.
    pub async fn replay_from(&self, from_sequence: i64) -> Result<Vec<Event>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT event_data FROM events
               WHERE sequence_number >= $1
               ORDER BY sequence_number"#,
            from_sequence,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value(r.event_data).ok())
            .collect())
    }

    // ── Snapshots ─────────────────────────────────────────────────────────────

    /// Write a snapshot of the current aggregate state at `sequence_number`.
    pub async fn write_snapshot(
        &self,
        aggregate_id: Uuid,
        sequence_number: i64,
    ) -> Result<(), sqlx::Error> {
        let events = self.load_up_to(aggregate_id, sequence_number).await?;
        let projection = CreatorProjection::from_events(&events);
        let data = serde_json::to_value(&projection).expect("Projection must be serializable");

        sqlx::query(
            "INSERT INTO event_snapshots (aggregate_id, sequence_number, snapshot_data)
             VALUES ($1, $2, $3)
             ON CONFLICT (aggregate_id, sequence_number) DO NOTHING",
        )
        .bind(aggregate_id)
        .bind(sequence_number)
        .bind(data)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Load the most recent snapshot for an aggregate.
    /// Returns `(sequence_number, projection)` or `None` if no snapshot exists.
    pub async fn load_latest_snapshot(
        &self,
        aggregate_id: Uuid,
    ) -> Result<Option<(i64, CreatorProjection)>, sqlx::Error> {
        let row: Option<(i64, serde_json::Value)> = sqlx::query_as(
            "SELECT sequence_number, snapshot_data FROM event_snapshots
             WHERE aggregate_id = $1
             ORDER BY sequence_number DESC
             LIMIT 1",
        )
        .bind(aggregate_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|(seq, data)| {
            serde_json::from_value(data).ok().map(|p| (seq, p))
        }))
    }
}
