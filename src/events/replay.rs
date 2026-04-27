use crate::events::{projections::CreatorProjection, store::EventStore};
use uuid::Uuid;

pub struct Replayer<'a> {
    store: &'a EventStore,
}

impl<'a> Replayer<'a> {
    pub fn new(store: &'a EventStore) -> Self {
        Self { store }
    }

    /// Rebuild the current state of a creator by replaying all its events,
    /// using the latest snapshot as a starting point when available.
    pub async fn creator_state(&self, creator_id: Uuid) -> Result<CreatorProjection, sqlx::Error> {
        // Try to load from the most recent snapshot + delta events.
        if let Some((snap_seq, mut projection)) =
            self.store.load_latest_snapshot(creator_id).await?
        {
            // Only replay events that occurred after the snapshot.
            let delta = self.store.load_from(creator_id, snap_seq + 1).await?;
            for event in &delta {
                projection.apply(event);
            }
            return Ok(projection);
        }

        // No snapshot — full replay.
        let events = self.store.load(creator_id).await?;
        Ok(CreatorProjection::from_events(&events))
    }

    /// Rebuild creator state as it was at a specific global sequence number (time-travel).
    /// Uses the most recent snapshot at or before `up_to_sequence` to minimise replay cost.
    pub async fn creator_state_at(
        &self,
        creator_id: Uuid,
        up_to_sequence: i64,
    ) -> Result<CreatorProjection, sqlx::Error> {
        // Load events scoped to this aggregate up to the requested sequence.
        let events = self
            .store
            .load_up_to(creator_id, up_to_sequence)
            .await?;
        Ok(CreatorProjection::from_events(&events))
    }
}
