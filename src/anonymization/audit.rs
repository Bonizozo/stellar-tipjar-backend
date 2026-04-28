//! Anonymization audit — records when and what data was anonymized.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AnonymizationRecord {
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: String,
    pub fields_anonymized: Vec<String>,
    pub reason: String,
    pub performed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

pub struct AnonymizationAudit {
    pool: PgPool,
}

impl AnonymizationAudit {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record(
        &self,
        entity_type: &str,
        entity_id: &str,
        fields: &[&str],
        reason: &str,
        performed_by: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        let fields: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
        sqlx::query!(
            r#"
            INSERT INTO anonymization_audit
                (id, entity_type, entity_id, fields_anonymized, reason, performed_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            Uuid::new_v4(),
            entity_type,
            entity_id,
            &fields,
            reason,
            performed_by,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        &self,
        entity_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AnonymizationRecord>, sqlx::Error> {
        match entity_type {
            Some(et) => sqlx::query_as::<_, AnonymizationRecord>(
                "SELECT * FROM anonymization_audit WHERE entity_type = $1 ORDER BY created_at DESC LIMIT $2",
            )
            .bind(et)
            .bind(limit)
            .fetch_all(&self.pool)
            .await,
            None => sqlx::query_as::<_, AnonymizationRecord>(
                "SELECT * FROM anonymization_audit ORDER BY created_at DESC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await,
        }
    }
}
