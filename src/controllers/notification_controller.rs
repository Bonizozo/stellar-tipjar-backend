use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::notification::{
    Notification, NotificationPreferences, UpdatePreferencesRequest,
};

/// Get preferences for a creator, creating defaults if none exist.
pub async fn get_preferences(pool: &PgPool, username: &str) -> AppResult<NotificationPreferences> {
    let prefs = sqlx::query_as::<_, NotificationPreferences>(
        r#"
        INSERT INTO notification_preferences (creator_username)
        VALUES ($1)
        ON CONFLICT (creator_username) DO UPDATE SET updated_at = notification_preferences.updated_at
        RETURNING creator_username, notify_on_tip, notify_on_milestone, updated_at
        "#,
    )
    .bind(username)
    .fetch_one(pool)
    .await?;
    Ok(prefs)
}

pub async fn update_preferences(
    pool: &PgPool,
    username: &str,
    req: UpdatePreferencesRequest,
) -> AppResult<NotificationPreferences> {
    let prefs = sqlx::query_as::<_, NotificationPreferences>(
        r#"
        INSERT INTO notification_preferences (creator_username, notify_on_tip, notify_on_milestone)
        VALUES ($1, COALESCE($2, TRUE), COALESCE($3, TRUE))
        ON CONFLICT (creator_username) DO UPDATE SET
            notify_on_tip       = COALESCE($2, notification_preferences.notify_on_tip),
            notify_on_milestone = COALESCE($3, notification_preferences.notify_on_milestone),
            updated_at          = NOW()
        RETURNING creator_username, notify_on_tip, notify_on_milestone, updated_at
        "#,
    )
    .bind(username)
    .bind(req.notify_on_tip)
    .bind(req.notify_on_milestone)
    .fetch_one(pool)
    .await?;
    Ok(prefs)
}

/// Persist a notification and return it.
pub async fn create_notification(
    pool: &PgPool,
    username: &str,
    notification_type: &str,
    payload: serde_json::Value,
) -> AppResult<Notification> {
    let n = sqlx::query_as::<_, Notification>(
        r#"
        INSERT INTO notifications (id, creator_username, type, payload)
        VALUES ($1, $2, $3, $4)
        RETURNING id, creator_username, type, payload, read, created_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(username)
    .bind(notification_type)
    .bind(payload)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn list_notifications(
    pool: &PgPool,
    username: &str,
    unread_only: bool,
) -> AppResult<Vec<Notification>> {
    let notifications = if unread_only {
        sqlx::query_as::<_, Notification>(
            "SELECT id, creator_username, type, payload, read, created_at
             FROM notifications WHERE creator_username = $1 AND read = FALSE
             ORDER BY created_at DESC LIMIT 100",
        )
        .bind(username)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Notification>(
            "SELECT id, creator_username, type, payload, read, created_at
             FROM notifications WHERE creator_username = $1
             ORDER BY created_at DESC LIMIT 100",
        )
        .bind(username)
        .fetch_all(pool)
        .await?
    };
    Ok(notifications)
}

pub async fn mark_read(pool: &PgPool, username: &str, notification_id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE notifications SET read = TRUE WHERE id = $1 AND creator_username = $2")
        .bind(notification_id)
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_all_read(pool: &PgPool, username: &str) -> AppResult<()> {
    sqlx::query("UPDATE notifications SET read = TRUE WHERE creator_username = $1")
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}
