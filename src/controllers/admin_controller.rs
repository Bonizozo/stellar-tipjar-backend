use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::admin::{AuditLog, StatsResponse};

// ── Statistics ────────────────────────────────────────────────────────────────

pub async fn get_stats(pool: &PgPool) -> AppResult<StatsResponse> {
    let total_creators = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM creators")
        .fetch_one(pool)
        .await?;

    let total_tips = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tips")
        .fetch_one(pool)
        .await?;

    // Sum amounts — stored as text, cast to numeric for aggregation.
    let total_volume: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT COALESCE(SUM(amount::numeric), 0)::text FROM tips",
    )
    .fetch_one(pool)
    .await?;

    Ok(StatsResponse {
        total_creators,
        total_tips,
        total_tip_volume_xlm: total_volume.unwrap_or_else(|| "0".to_string()),
    })
}

// ── Creator moderation ────────────────────────────────────────────────────────

pub async fn delete_creator(pool: &PgPool, username: &str) -> AppResult<bool> {
    let result = sqlx::query("DELETE FROM creators WHERE username = $1")
        .bind(username)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

// ── Audit log ─────────────────────────────────────────────────────────────────

pub async fn write_audit_log(
    pool: &PgPool,
    admin_username: &str,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<&str>,
    detail: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_logs (id, admin_username, action, target_type, target_id, detail, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(admin_username)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(detail)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_audit_logs(pool: &PgPool, limit: i64) -> AppResult<Vec<AuditLog>> {
    let logs = sqlx::query_as::<_, AuditLog>(
        r#"
        SELECT id, admin_username, action, target_type, target_id, detail, created_at
        FROM audit_logs
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(logs)
}

// ── Admin user lookup (for resolving key → username in middleware) ─────────────

pub async fn get_admin_username_by_key_hash(
    pool: &PgPool,
    key_hash: &str,
) -> AppResult<Option<String>> {
    let username = sqlx::query_scalar::<_, String>(
        "SELECT username FROM admin_users WHERE api_key_hash = $1",
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;

    Ok(username)
}
