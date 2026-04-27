use crate::events::types::Event;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Read-model built by folding events for a single creator.
#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreatorProjection {
    pub id: Option<Uuid>,
    pub username: String,
    pub wallet_address: String,
    pub tip_count: u64,
    pub registered_at: Option<DateTime<Utc>>,
}

impl CreatorProjection {
    /// Apply a single event, mutating the projection in place.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::CreatorRegistered {
                id,
                username,
                wallet_address,
                timestamp,
            } => {
                self.id = Some(*id);
                self.username = username.clone();
                self.wallet_address = wallet_address.clone();
                self.registered_at = Some(*timestamp);
            }
            Event::TipReceived { .. } => {
                self.tip_count += 1;
            }
        }
    }

    /// Build a projection by replaying a slice of events.
    pub fn from_events(events: &[Event]) -> Self {
        let mut p = Self::default();
        for e in events {
            p.apply(e);
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg_event(id: Uuid) -> Event {
        Event::CreatorRegistered {
            id,
            username: "alice".into(),
            wallet_address: "GABC".into(),
            timestamp: Utc::now(),
        }
    }

    fn tip_event(creator_id: Uuid) -> Event {
        Event::TipReceived {
            id: Uuid::new_v4(),
            creator_id,
            amount: "5.0".into(),
            transaction_hash: "a".repeat(64),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_projection_from_events() {
        let id = Uuid::new_v4();
        let events = vec![reg_event(id), tip_event(id), tip_event(id)];
        let p = CreatorProjection::from_events(&events);
        assert_eq!(p.username, "alice");
        assert_eq!(p.tip_count, 2);
        assert_eq!(p.id, Some(id));
    }

    #[test]
    fn test_empty_events_gives_default() {
        let p = CreatorProjection::from_events(&[]);
        assert_eq!(p, CreatorProjection::default());
    }
}
