use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::{ContentType, ModerationResult};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ModerationQueueItem {
    pub id: Uuid,
    pub content_type: String,
    pub content_id: Option<Uuid>,
    pub content_text: String,
    pub flags: serde_json::Value,
    pub status: String,
    pub action: Option<String>,
    pub flagged_by: Option<String>,
    pub flag_reason: Option<String>,
    pub ai_score: Option<f64>,
    pub ai_reasoning: Option<String>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ModerationFlag {
    pub id: Uuid,
    pub content_type: String,
    pub content_id: Uuid,
    pub content_text: String,
    pub reason: String,
    pub flagged_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ModerationHistoryEntry {
    pub id: Uuid,
    pub queue_item_id: Uuid,
    pub action: String,
    pub performed_by: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ModerationStats {
    pub pending: i64,
    pub approved: i64,
    pub rejected: i64,
    pub total: i64,
}

pub struct ReviewQueue {
    db: PgPool,
}

impl ReviewQueue {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Persist an auto-detected flagged item to the review queue.
    pub async fn enqueue(
        &self,
        content: &str,
        content_type: &ContentType,
        content_id: Option<Uuid>,
        result: &ModerationResult,
    ) -> anyhow::Result<Uuid> {
        let flags = serde_json::to_value(&result.violations)
            .unwrap_or(serde_json::Value::Array(vec![]));

        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO moderation_queue
                (id, content_type, content_id, content_text, flags, status, ai_score, ai_reasoning, created_at)
             VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7, NOW())
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(content_type.as_str())
        .bind(content_id)
        .bind(content)
        .bind(flags)
        .bind(result.ai_score.map(|s| s as f64))
        .bind(&result.ai_reasoning)
        .fetch_one(&self.db)
        .await?;

        Ok(id)
    }

    /// Manually flag content and enqueue it for review.
    pub async fn flag(
        &self,
        content_type: &str,
        content_id: Uuid,
        content_text: &str,
        reason: &str,
        flagged_by: &str,
    ) -> anyhow::Result<Uuid> {
        // Record the flag
        sqlx::query(
            "INSERT INTO moderation_flags (content_type, content_id, content_text, reason, flagged_by)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(content_type)
        .bind(content_id)
        .bind(content_text)
        .bind(reason)
        .bind(flagged_by)
        .execute(&self.db)
        .await?;

        // Enqueue for review (upsert: skip if already pending for this content)
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO moderation_queue
                (id, content_type, content_id, content_text, flags, status, flagged_by, flag_reason, created_at)
             VALUES ($1, $2, $3, $4, '[]'::jsonb, 'pending', $5, $6, NOW())
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(content_type)
        .bind(content_id)
        .bind(content_text)
        .bind(flagged_by)
        .bind(reason)
        .fetch_optional(&self.db)
        .await?
        .unwrap_or_else(Uuid::new_v4);

        Ok(id)
    }

    /// List items, optionally filtered by status.
    pub async fn list(
        &self,
        status: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<ModerationQueueItem>> {
        let items = match status {
            Some(s) => sqlx::query_as::<_, ModerationQueueItem>(
                "SELECT id, content_type, content_id, content_text, flags, status, action,
                        flagged_by, flag_reason, ai_score, ai_reasoning,
                        reviewed_by, reviewed_at, created_at
                 FROM moderation_queue WHERE status = $1
                 ORDER BY created_at DESC LIMIT $2",
            )
            .bind(s)
            .bind(limit)
            .fetch_all(&self.db)
            .await?,
            None => sqlx::query_as::<_, ModerationQueueItem>(
                "SELECT id, content_type, content_id, content_text, flags, status, action,
                        flagged_by, flag_reason, ai_score, ai_reasoning,
                        reviewed_by, reviewed_at, created_at
                 FROM moderation_queue
                 ORDER BY created_at DESC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&self.db)
            .await?,
        };
        Ok(items)
    }

    pub async fn approve(&self, id: Uuid, reviewed_by: &str) -> anyhow::Result<bool> {
        self.apply_action(id, "approved", "approve", reviewed_by, None).await
    }

    pub async fn reject(&self, id: Uuid, reviewed_by: &str) -> anyhow::Result<bool> {
        self.apply_action(id, "rejected", "reject", reviewed_by, None).await
    }

    pub async fn dismiss(&self, id: Uuid, reviewed_by: &str, note: Option<&str>) -> anyhow::Result<bool> {
        self.apply_action(id, "approved", "dismiss", reviewed_by, note).await
    }

    pub async fn warn(&self, id: Uuid, reviewed_by: &str, note: Option<&str>) -> anyhow::Result<bool> {
        self.apply_action(id, "rejected", "warn", reviewed_by, note).await
    }

    pub async fn ban(&self, id: Uuid, reviewed_by: &str, note: Option<&str>) -> anyhow::Result<bool> {
        self.apply_action(id, "rejected", "ban", reviewed_by, note).await
    }

    /// Core action applier: updates queue row + writes history entry atomically.
    async fn apply_action(
        &self,
        id: Uuid,
        status: &str,
        action: &str,
        reviewed_by: &str,
        note: Option<&str>,
    ) -> anyhow::Result<bool> {
        let mut tx = self.db.begin().await?;

        let rows = sqlx::query(
            "UPDATE moderation_queue
             SET status = $1, action = $2, reviewed_by = $3, reviewed_at = NOW()
             WHERE id = $4 AND status = 'pending'",
        )
        .bind(status)
        .bind(action)
        .bind(reviewed_by)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if rows > 0 {
            sqlx::query(
                "INSERT INTO moderation_history (queue_item_id, action, performed_by, note)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(id)
            .bind(action)
            .bind(reviewed_by)
            .bind(note)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(rows > 0)
    }

    /// Full history for a single queue item.
    pub async fn history(&self, queue_item_id: Uuid) -> anyhow::Result<Vec<ModerationHistoryEntry>> {
        let entries = sqlx::query_as::<_, ModerationHistoryEntry>(
            "SELECT id, queue_item_id, action, performed_by, note, created_at
             FROM moderation_history WHERE queue_item_id = $1
             ORDER BY created_at ASC",
        )
        .bind(queue_item_id)
        .fetch_all(&self.db)
        .await?;
        Ok(entries)
    }

    pub async fn stats(&self) -> anyhow::Result<ModerationStats> {
        let pending: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM moderation_queue WHERE status = 'pending'")
                .fetch_one(&self.db)
                .await?;
        let approved: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM moderation_queue WHERE status = 'approved'")
                .fetch_one(&self.db)
                .await?;
        let rejected: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM moderation_queue WHERE status = 'rejected'")
                .fetch_one(&self.db)
                .await?;
        Ok(ModerationStats {
            pending,
            approved,
            rejected,
            total: pending + approved + rejected,
        })
    }
}
