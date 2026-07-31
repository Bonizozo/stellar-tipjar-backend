use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EmailPreferences {
    pub creator_id: Uuid,
    pub tip_notifications: bool,
    pub weekly_summary: bool,
    pub marketing_emails: bool,
    pub unsubscribed_at: Option<chrono::NaiveDateTime>,
}

impl EmailPreferences {
    pub async fn get(pool: &PgPool, creator_id: Uuid) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            EmailPreferences,
            r#"
            SELECT creator_id, tip_notifications, weekly_summary, marketing_emails, unsubscribed_at
            FROM email_preferences
            WHERE creator_id = $1
            "#,
            creator_id
        )
        .fetch_one(pool)
        .await
    }

    pub async fn create_default(pool: &PgPool, creator_id: Uuid) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            EmailPreferences,
            r#"
            INSERT INTO email_preferences (creator_id, tip_notifications, weekly_summary, marketing_emails)
            VALUES ($1, true, true, true)
            ON CONFLICT (creator_id) DO NOTHING
            RETURNING creator_id, tip_notifications, weekly_summary, marketing_emails, unsubscribed_at
            "#,
            creator_id
        )
        .fetch_one(pool)
        .await
    }

    pub async fn update(
        pool: &PgPool,
        creator_id: Uuid,
        tip_notifications: Option<bool>,
        weekly_summary: Option<bool>,
        marketing_emails: Option<bool>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            EmailPreferences,
            r#"
            UPDATE email_preferences
            SET 
                tip_notifications = COALESCE($2, tip_notifications),
                weekly_summary = COALESCE($3, weekly_summary),
                marketing_emails = COALESCE($4, marketing_emails)
            WHERE creator_id = $1
            RETURNING creator_id, tip_notifications, weekly_summary, marketing_emails, unsubscribed_at
            "#,
            creator_id,
            tip_notifications,
            weekly_summary,
            marketing_emails
        )
        .fetch_one(pool)
        .await
    }

    pub async fn unsubscribe_all(pool: &PgPool, creator_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE email_preferences
            SET 
                tip_notifications = false,
                weekly_summary = false,
                marketing_emails = false,
                unsubscribed_at = NOW()
            WHERE creator_id = $1
            "#,
            creator_id
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
