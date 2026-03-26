use sqlx::PgPool;

use crate::errors::AppResult;

/// Multiplier above the creator's average that triggers an anomaly alert.
const SPIKE_THRESHOLD: u64 = 10;

/// Checks whether `amount_stroops` is anomalously large relative to the
/// creator's historical average. If so, persists a record to `anomaly_log`.
pub async fn check_and_log(
    pool: &PgPool,
    creator_username: &str,
    amount_stroops: u64,
) -> AppResult<bool> {
    let (tip_count, avg): (i64, i64) = sqlx::query_as(
        "SELECT tip_count, avg_amount_stroops FROM creator_stats WHERE creator_username = $1",
    )
    .bind(creator_username)
    .fetch_optional(pool)
    .await?
    .unwrap_or((0, 0));

    // Need at least a few data points before flagging anomalies.
    if tip_count < 5 || avg == 0 {
        return Ok(false);
    }

    let is_spike = amount_stroops > (avg as u64).saturating_mul(SPIKE_THRESHOLD);
    if is_spike {
        sqlx::query(
            "INSERT INTO anomaly_log (creator_username, amount_stroops, baseline_stroops) VALUES ($1, $2, $3)",
        )
        .bind(creator_username)
        .bind(amount_stroops as i64)
        .bind(avg)
        .execute(pool)
        .await?;

        tracing::warn!(
            creator = creator_username,
            amount = amount_stroops,
            baseline = avg,
            "Anomaly detected: tip spike"
        );
    }

    Ok(is_spike)
}
