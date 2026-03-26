use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// All domain events in the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    CreatorRegistered {
        id: Uuid,
        username: String,
        wallet_address: String,
        timestamp: DateTime<Utc>,
    },
    TipReceived {
        id: Uuid,
        creator_id: Uuid,
        amount: String,
        transaction_hash: String,
        timestamp: DateTime<Utc>,
    },
}

impl Event {
    pub fn event_type(&self) -> &'static str {
        match self {
            Event::CreatorRegistered { .. } => "creator_registered",
            Event::TipReceived { .. }       => "tip_received",
        }
    }

    pub fn aggregate_id(&self) -> Uuid {
        match self {
            Event::CreatorRegistered { id, .. } => *id,
            Event::TipReceived { creator_id, .. } => *creator_id,
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Event::CreatorRegistered { timestamp, .. } => *timestamp,
            Event::TipReceived { timestamp, .. }       => *timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creator_event() -> Event {
        Event::CreatorRegistered {
            id: Uuid::nil(),
            username: "alice".into(),
            wallet_address: "GABC".into(),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_event_type_label() {
        assert_eq!(creator_event().event_type(), "creator_registered");
    }

    #[test]
    fn test_aggregate_id() {
        assert_eq!(creator_event().aggregate_id(), Uuid::nil());
    }

    #[test]
    fn test_roundtrip_serialization() {
        let ev = creator_event();
        let json = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
