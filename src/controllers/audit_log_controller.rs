use sqlx::PgPool;

use crate::errors::AppResult;
use crate::models::audit_log::{AuditLog, AuditLogQuery};

/// Record an audit log entry.
pub async fn log(
    pool: &PgPool,
    event_type: &str,
    actor: Option<&str>,
    resource: &str,
    resource_id: Option<&str>,
    action: &str,
    before_data: Option<serde_json::Value>,
    after_data: Option<serde_json::Value>,
    metadata: serde_json::Value,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_logs
            (event_type, actor, resource, resource_id, action, before_data, after_data, metadata, ip_address, user_agent)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(event_type)
    .bind(actor)
    .bind(resource)
    .bind(resource_id)
    .bind(action)
    .bind(before_data)
    .bind(after_data)
    .bind(metadata)
    .bind(ip_address)
    .bind(user_agent)
    .execute(pool)
    .await?;
    Ok(())
}

/// Search audit logs with optional filters.
pub async fn search(pool: &PgPool, query: &AuditLogQuery) -> AppResult<Vec<AuditLog>> {
    let limit = query.clamped_limit();

    let logs = sqlx::query_as::<_, AuditLog>(
        r#"
        SELECT id, event_type, actor, resource, resource_id, action,
               before_data, after_data, metadata, ip_address, user_agent, created_at
        FROM audit_logs
        WHERE ($1::TEXT IS NULL OR event_type = $1)
          AND ($2::TEXT IS NULL OR actor = $2)
          AND ($3::TEXT IS NULL OR resource = $3)
          AND ($4::TIMESTAMPTZ IS NULL OR created_at >= $4)
          AND ($5::TIMESTAMPTZ IS NULL OR created_at <= $5)
        ORDER BY created_at DESC
        LIMIT $6 OFFSET $7
        "#,
    )
    .bind(&query.event_type)
    .bind(&query.actor)
    .bind(&query.resource)
    .bind(query.from)
    .bind(query.to)
    .bind(limit)
    .bind(query.offset)
    .fetch_all(pool)
    .await?;

    Ok(logs)
}

/// Delete audit logs older than the given number of days (retention policy).
pub async fn purge_old_logs(pool: &PgPool, retain_days: i64) -> AppResult<u64> {
    let result = sqlx::query(
        "DELETE FROM audit_logs WHERE created_at < NOW() - ($1 || ' days')::INTERVAL",
    )
    .bind(retain_days)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
