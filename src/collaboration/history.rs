//! Collaboration history — persists and retrieves operation history.

use crate::collaboration::crdt::Operation;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub document_id: Uuid,
    pub user_id: Uuid,
    pub op_type: String,
    pub position: i64,
    pub content: String,
    pub length: i64,
    pub clock: i64,
    pub created_at: DateTime<Utc>,
}

pub struct CollaborationHistory {
    pool: PgPool,
}

impl CollaborationHistory {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record(&self, op: &Operation) -> Result<(), sqlx::Error> {
        let op_type = format!("{:?}", op.op_type).to_lowercase();
        sqlx::query!(
            r#"
            INSERT INTO collaboration_history
                (id, document_id, user_id, op_type, position, content, length, clock, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            op.id,
            op.session_id,
            op.user_id,
            op_type,
            op.position as i64,
            op.content,
            op.length as i64,
            op.clock as i64,
            op.timestamp,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        &self,
        document_id: Uuid,
        since: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<Vec<HistoryEntry>, sqlx::Error> {
        let since = since.unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
        sqlx::query_as::<_, HistoryEntry>(
            r#"
            SELECT * FROM collaboration_history
            WHERE document_id = $1 AND created_at >= $2
            ORDER BY clock ASC
            LIMIT $3
            "#,
        )
        .bind(document_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
}
