use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "email_status", rename_all = "lowercase")]
pub enum EmailStatus {
    Pending,
    Sent,
    Delivered,
    Bounced,
    Complained,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EmailDelivery {
    pub id: Uuid,
    pub creator_id: Uuid,
    pub email_type: String,
    pub recipient: String,
    pub subject: String,
    pub status: EmailStatus,
    pub error_message: Option<String>,
    pub sent_at: Option<chrono::NaiveDateTime>,
    pub delivered_at: Option<chrono::NaiveDateTime>,
    pub bounced_at: Option<chrono::NaiveDateTime>,
    pub created_at: chrono::NaiveDateTime,
}

impl EmailDelivery {
    pub async fn create(
        pool: &PgPool,
        creator_id: Uuid,
        email_type: &str,
        recipient: &str,
        subject: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();

        sqlx::query!(
            r#"
            INSERT INTO email_deliveries (id, creator_id, email_type, recipient, subject, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            "#,
            id,
            creator_id,
            email_type,
            recipient,
            subject
        )
        .execute(pool)
        .await?;

        Ok(id)
    }

    pub async fn mark_sent(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE email_deliveries
            SET status = 'sent', sent_at = $2
            WHERE id = $1
            "#,
            id,
            Utc::now().naive_utc()
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_delivered(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE email_deliveries
            SET status = 'delivered', delivered_at = $2
            WHERE id = $1
            "#,
            id,
            Utc::now().naive_utc()
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_bounced(pool: &PgPool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE email_deliveries
            SET status = 'bounced', bounced_at = $2, error_message = $3
            WHERE id = $1
            "#,
            id,
            Utc::now().naive_utc(),
            error
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_complained(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE email_deliveries
            SET status = 'complained'
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(pool: &PgPool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE email_deliveries
            SET status = 'failed', error_message = $2
            WHERE id = $1
            "#,
            id,
            error
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_creator(
        pool: &PgPool,
        creator_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            EmailDelivery,
            r#"
            SELECT 
                id, creator_id, email_type, recipient, subject,
                status as "status: EmailStatus",
                error_message, sent_at, delivered_at, bounced_at, created_at
            FROM email_deliveries
            WHERE creator_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
            creator_id,
            limit
        )
        .fetch_all(pool)
        .await
    }
}
