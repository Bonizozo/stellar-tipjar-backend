//! CRDT-based operational transformation for real-time collaboration.
//!
//! Uses a last-write-wins register with vector clocks for conflict resolution.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Type of operation applied to a document
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Insert,
    Delete,
    Update,
}

/// A single collaborative operation with vector clock for ordering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: Uuid,
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub op_type: OperationType,
    /// Position in the document (character offset)
    pub position: usize,
    /// Content for Insert/Update; empty for Delete
    pub content: String,
    /// Number of characters to delete (for Delete ops)
    pub length: usize,
    /// Logical clock value for this user
    pub clock: u64,
    pub timestamp: DateTime<Utc>,
}

impl Operation {
    /// Transform `self` against a concurrent `other` operation (OT).
    /// Adjusts position so both ops can be applied independently.
    pub fn transform_against(&self, other: &Operation) -> Operation {
        let mut transformed = self.clone();

        if other.timestamp >= self.timestamp {
            return transformed;
        }

        match (&self.op_type, &other.op_type) {
            (OperationType::Insert, OperationType::Insert) => {
                if other.position <= self.position {
                    transformed.position += other.content.len();
                }
            }
            (OperationType::Insert, OperationType::Delete) => {
                if other.position < self.position {
                    transformed.position =
                        self.position.saturating_sub(other.length);
                }
            }
            (OperationType::Delete, OperationType::Insert) => {
                if other.position <= self.position {
                    transformed.position += other.content.len();
                }
            }
            (OperationType::Delete, OperationType::Delete) => {
                if other.position < self.position {
                    transformed.position =
                        self.position.saturating_sub(other.length);
                }
            }
            _ => {}
        }

        transformed
    }
}

/// Shared document state with operation log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentState {
    pub content: String,
    pub version: u64,
    pub last_modified: DateTime<Utc>,
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            content: String::new(),
            version: 0,
            last_modified: Utc::now(),
        }
    }
}

impl DocumentState {
    /// Apply an operation to the document, returning the new state.
    pub fn apply(&mut self, op: &Operation) {
        match op.op_type {
            OperationType::Insert => {
                let pos = op.position.min(self.content.len());
                self.content.insert_str(pos, &op.content);
            }
            OperationType::Delete => {
                let start = op.position.min(self.content.len());
                let end = (op.position + op.length).min(self.content.len());
                self.content.drain(start..end);
            }
            OperationType::Update => {
                let start = op.position.min(self.content.len());
                let end = (op.position + op.length).min(self.content.len());
                self.content.drain(start..end);
                self.content.insert_str(start, &op.content);
            }
        }
        self.version += 1;
        self.last_modified = Utc::now();
    }
}

/// A collaboration session for a shared document
pub struct CollaborationSession {
    pub session_id: Uuid,
    pub document_id: Uuid,
    document: RwLock<DocumentState>,
    /// Ordered operation log for history and replay
    operations: RwLock<Vec<Operation>>,
    /// Per-user vector clocks
    clocks: RwLock<HashMap<Uuid, u64>>,
}

impl CollaborationSession {
    pub fn new(document_id: Uuid) -> Arc<Self> {
        Arc::new(Self {
            session_id: Uuid::new_v4(),
            document_id,
            document: RwLock::new(DocumentState::default()),
            operations: RwLock::new(Vec::new()),
            clocks: RwLock::new(HashMap::new()),
        })
    }

    /// Submit an operation, applying OT against any concurrent ops, then persist.
    pub async fn submit(&self, mut op: Operation) -> Operation {
        let mut ops = self.operations.write().await;
        let mut doc = self.document.write().await;
        let mut clocks = self.clocks.write().await;

        // Advance user clock
        let clock = clocks.entry(op.user_id).or_insert(0);
        *clock += 1;
        op.clock = *clock;

        // Transform against all ops submitted after the client's known version
        let concurrent: Vec<Operation> = ops
            .iter()
            .filter(|o| o.user_id != op.user_id && o.timestamp > op.timestamp)
            .cloned()
            .collect();

        for concurrent_op in &concurrent {
            op = op.transform_against(concurrent_op);
        }

        doc.apply(&op);
        ops.push(op.clone());

        tracing::debug!(
            session_id = %self.session_id,
            op_id = %op.id,
            user_id = %op.user_id,
            version = doc.version,
            "Operation applied"
        );

        op
    }

    pub async fn document_state(&self) -> DocumentState {
        self.document.read().await.clone()
    }

    pub async fn operations_since(&self, version: u64) -> Vec<Operation> {
        let ops = self.operations.read().await;
        ops.iter()
            .skip(version as usize)
            .cloned()
            .collect()
    }
}

/// Registry of active collaboration sessions
pub struct SessionRegistry {
    sessions: RwLock<HashMap<Uuid, Arc<CollaborationSession>>>,
}

impl SessionRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: RwLock::new(HashMap::new()),
        })
    }

    pub async fn get_or_create(&self, document_id: Uuid) -> Arc<CollaborationSession> {
        {
            let sessions = self.sessions.read().await;
            if let Some(s) = sessions.get(&document_id) {
                return Arc::clone(s);
            }
        }
        let session = CollaborationSession::new(document_id);
        let mut sessions = self.sessions.write().await;
        sessions.insert(document_id, Arc::clone(&session));
        session
    }

    pub async fn get(&self, document_id: &Uuid) -> Option<Arc<CollaborationSession>> {
        self.sessions.read().await.get(document_id).cloned()
    }
}
