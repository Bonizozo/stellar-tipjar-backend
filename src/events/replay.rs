use uuid::Uuid;
use crate::events::{store::EventStore, projections::CreatorProjection};

pub struct Replayer<'a> {
    store: &'a EventStore,
}

impl<'a> Replayer<'a> {
    pub fn new(store: &'a EventStore) -> Self {
        Self { store }
    }

    /// Rebuild the current state of a creator by replaying all its events.
    pub async fn creator_state(&self, creator_id: Uuid) -> Result<CreatorProjection, sqlx::Error> {
        let events = self.store.load(creator_id).await?;
        Ok(CreatorProjection::from_events(&events))
    }

    /// Rebuild creator state as it was at a specific sequence number (time-travel).
    pub async fn creator_state_at(
        &self,
        creator_id: Uuid,
        up_to_sequence: i64,
    ) -> Result<CreatorProjection, sqlx::Error> {
        let all = self.store.load(creator_id).await?;
        // We need sequence numbers, so we replay from the full set and cap by count.
        // A production implementation would pass the sequence cap to the DB query.
        let capped: Vec<_> = self
            .store
            .replay_from(0)
            .await?
            .into_iter()
            .filter(|e| e.aggregate_id() == creator_id)
            .take_while({
                let mut seq = 0i64;
                move |_| {
                    seq += 1;
                    seq <= up_to_sequence
                }
            })
            .collect();

        // Prefer the pre-loaded full set if no cap was needed.
        let events = if up_to_sequence >= all.len() as i64 { all } else { capped };
        Ok(CreatorProjection::from_events(&events))
    }
}
