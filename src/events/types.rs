use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Schema version for each event variant.
/// Increment when the payload shape changes to enable upcasting.
pub const EVENT_VERSION: i32 = 1;

/// All domain events in the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    CreatorRegistered {
        /// Schema version — allows upcasting old payloads.
        #[serde(default = "default_version")]
        version: i32,
        id: Uuid,
        username: String,
        wallet_address: String,
        timestamp: DateTime<Utc>,
    },
    TipReceived {
        #[serde(default = "default_version")]
        version: i32,
        id: Uuid,
        creator_id: Uuid,
        amount: String,
        transaction_hash: String,
        timestamp: DateTime<Utc>,
    },
}

fn default_version() -> i32 {
    EVENT_VERSION
}

impl Event {
    pub fn event_type(&self) -> &'static str {
        match self {
            Event::CreatorRegistered { .. } => "creator_registered",
            Event::TipReceived { .. } => "tip_received",
        }
    }

    pub fn version(&self) -> i32 {
        match self {
            Event::CreatorRegistered { version, .. } => *version,
            Event::TipReceived { version, .. } => *version,
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
            Event::TipReceived { timestamp, .. } => *timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creator_event() -> Event {
        Event::CreatorRegistered {
            version: EVENT_VERSION,
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
    fn test_version() {
        assert_eq!(creator_event().version(), EVENT_VERSION);
    }

    #[test]
    fn test_roundtrip_serialization() {
        let ev = creator_event();
        let json = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn test_old_event_without_version_defaults() {
        // Simulate a stored event that predates versioning (no "version" field).
        let json = r#"{"type":"creator_registered","id":"00000000-0000-0000-0000-000000000000","username":"alice","wallet_address":"GABC","timestamp":"2024-01-01T00:00:00Z"}"#;
        let ev: Event = serde_json::from_str(json).unwrap();
        assert_eq!(ev.version(), EVENT_VERSION);
    }
}
