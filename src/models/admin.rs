use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── DB rows ──────────────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
pub struct AdminUser {
    pub id: Uuid,
    pub username: String,
    pub api_key_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AuditLog {
    pub id: Uuid,
    pub admin_username: String,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Responses ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub total_creators: i64,
    pub total_tips: i64,
    pub total_tip_volume_xlm: String,
}

#[derive(Debug, Serialize)]
pub struct AuditLogResponse {
    pub id: Uuid,
    pub admin_username: String,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<AuditLog> for AuditLogResponse {
    fn from(l: AuditLog) -> Self {
        Self {
            id: l.id,
            admin_username: l.admin_username,
            action: l.action,
            target_type: l.target_type,
            target_id: l.target_id,
            detail: l.detail,
            created_at: l.created_at,
        }
    }
}

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeleteCreatorRequest {
    pub reason: Option<String>,
}
