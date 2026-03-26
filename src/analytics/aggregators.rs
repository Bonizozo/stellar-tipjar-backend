use sqlx::PgPool;
use chrono::{DateTime, Utc};

use crate::errors::AppResult;

/// Upserts per-creator aggregate stats into `creator_stats`.
pub async fn update_creator_stats(pool: &PgPool, creator_username: &str, amount_stroops: u64) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO creator_stats (creator_username, tip_count, total_amount_stroops, avg_amount_stroops, last_tip_at, updated_at)
        VALUES ($1, 1, $2, $2, NOW(), NOW())
        ON CONFLICT (creator_username) DO UPDATE SET
            tip_count            = creator_stats.tip_count + 1,
            total_amount_stroops = creator_stats.total_amount_stroops + $2,
            avg_amount_stroops   = (creator_stats.total_amount_stroops + $2) / (creator_stats.tip_count + 1),
            last_tip_at          = NOW(),
            updated_at           = NOW()
        "#,
    )
    .bind(creator_username)
    .bind(amount_stroops as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns (tip_count, avg_amount_stroops) for a creator, or (0, 0) if none.
pub async fn get_creator_stats(pool: &PgPool, creator_username: &str) -> AppResult<(i64, i64)> {
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT tip_count, avg_amount_stroops FROM creator_stats WHERE creator_username = $1",
    )
    .bind(creator_username)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or((0, 0)))
}
