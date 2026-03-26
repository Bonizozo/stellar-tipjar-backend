use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SecurityAuditLog {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub resource: String,
    pub details: serde_json::Value,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub struct AuditLogger {
    pub pool: PgPool,
}

impl AuditLogger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn log(
        &self,
        user_id: Option<Uuid>,
        action: &str,
        resource: &str,
        details: serde_json::Value,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO security_audit_logs (user_id, action, resource, details, ip_address, user_agent)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user_id)
        .bind(action)
        .bind(resource)
        .bind(details)
        .bind(ip_address)
        .bind(user_agent)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_trail(
        &self,
        user_id: Option<Uuid>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<SecurityAuditLog>> {
        let logs = sqlx::query_as::<_, SecurityAuditLog>(
            "SELECT id, user_id, action, resource, details, ip_address, user_agent, created_at
             FROM security_audit_logs
             WHERE ($1::uuid IS NULL OR user_id = $1)
               AND created_at BETWEEN $2 AND $3
             ORDER BY created_at DESC
             LIMIT $4",
        )
        .bind(user_id)
        .bind(from)
        .bind(to)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(logs)
    }
}
