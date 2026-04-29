use sqlx::PgPool;

use crate::errors::AppResult;

/// Minimum number of tips in a window before alerts fire.
const MIN_WINDOW_TIPS: i64 = 3;
/// Volume threshold (stroops) in a tumbling window that triggers a high-volume alert.
const HIGH_VOLUME_THRESHOLD_STROOPS: i64 = 1_000_000_000; // 100 XLM

/// Evaluate alert conditions for a creator after each tip event.
/// Inserts a row into `analytics_alerts` when a threshold is breached.
pub async fn evaluate(
    pool: &PgPool,
    creator_username: &str,
    window_total_stroops: i64,
    window_tip_count: i64,
) -> AppResult<()> {
    if window_tip_count < MIN_WINDOW_TIPS {
        return Ok(());
    }

    if window_total_stroops >= HIGH_VOLUME_THRESHOLD_STROOPS {
        sqlx::query(
            "INSERT INTO analytics_alerts (creator_username, alert_type, value_stroops)
             VALUES ($1, 'high_volume_window', $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(creator_username)
        .bind(window_total_stroops)
        .execute(pool)
        .await?;

        tracing::warn!(
            creator = creator_username,
            window_total = window_total_stroops,
            "Alert: high-volume tip window"
        );
    }

    Ok(())
}
