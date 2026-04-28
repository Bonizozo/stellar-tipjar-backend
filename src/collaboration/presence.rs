//! Presence awareness — tracks which users are active in a collaboration session.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

const PRESENCE_TTL_SECS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPresence {
    pub user_id: Uuid,
    pub username: String,
    pub document_id: Uuid,
    /// Cursor position in the document
    pub cursor_position: Option<usize>,
    pub last_seen: DateTime<Utc>,
    pub is_active: bool,
}

impl UserPresence {
    pub fn is_stale(&self) -> bool {
        let age = Utc::now()
            .signed_duration_since(self.last_seen)
            .num_seconds();
        age > PRESENCE_TTL_SECS
    }
}

/// Manages user presence across collaboration sessions
pub struct PresenceManager {
    /// document_id -> user_id -> presence
    presence: RwLock<HashMap<Uuid, HashMap<Uuid, UserPresence>>>,
}

impl PresenceManager {
    pub fn new() -> Arc<Self> {
        let mgr = Arc::new(Self {
            presence: RwLock::new(HashMap::new()),
        });
        // Spawn background cleanup of stale presence entries
        let mgr_clone = Arc::clone(&mgr);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                mgr_clone.cleanup_stale().await;
            }
        });
        mgr
    }

    pub async fn heartbeat(
        &self,
        document_id: Uuid,
        user_id: Uuid,
        username: String,
        cursor_position: Option<usize>,
    ) {
        let mut presence = self.presence.write().await;
        let doc_presence = presence.entry(document_id).or_default();
        doc_presence.insert(
            user_id,
            UserPresence {
                user_id,
                username,
                document_id,
                cursor_position,
                last_seen: Utc::now(),
                is_active: true,
            },
        );
    }

    pub async fn leave(&self, document_id: Uuid, user_id: Uuid) {
        let mut presence = self.presence.write().await;
        if let Some(doc_presence) = presence.get_mut(&document_id) {
            doc_presence.remove(&user_id);
        }
    }

    pub async fn active_users(&self, document_id: &Uuid) -> Vec<UserPresence> {
        let presence = self.presence.read().await;
        presence
            .get(document_id)
            .map(|m| {
                m.values()
                    .filter(|p| !p.is_stale())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn cleanup_stale(&self) {
        let mut presence = self.presence.write().await;
        for doc_presence in presence.values_mut() {
            doc_presence.retain(|_, p| !p.is_stale());
        }
        presence.retain(|_, m| !m.is_empty());
    }
}
