use std::time::Duration;

use serde::de::DeserializeOwned;
use sqlx::PgPool;

use crate::cache::{keys, redis_client};
use crate::db::connection::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::stats::{
    CreatorStats, StatsQuery, TipDailyStat, TipHistoryItem, TipSummary, TopSupporter,
};

const CREATOR_STATS_TTL_SECS: u64 = redis_client::TTL_CREATOR;

#[derive(Debug, sqlx::FromRow)]
struct CreatorStatsRow {
    creator_username: String,
    total_amount_xlm: String,
    tip_count: i64,
    unique_supporters: i64,
    top_supporters: String,
    tip_history: String,
}

impl CreatorStatsRow {
    fn into_stats(self) -> AppResult<CreatorStats> {
        let top_supporters =
            parse_json_array::<TopSupporter>(&self.top_supporters, "top_supporters")?;
        let tip_history = parse_json_array::<TipHistoryItem>(&self.tip_history, "tip_history")?;

        Ok(CreatorStats {
            creator_username: self.creator_username,
            total_amount_xlm: self.total_amount_xlm,
            tip_count: self.tip_count,
            unique_supporters: self.unique_supporters,
            top_supporters,
            tip_history,
        })
    }
}

fn parse_json_array<T>(raw: &str, field: &'static str) -> AppResult<Vec<T>>
where
    T: DeserializeOwned,
{
    serde_json::from_str(raw).map_err(|e| {
        AppError::internal_with_message(format!(
            "Failed to decode creator stats field '{field}': {e}"
        ))
    })
}

pub async fn get_creator_summary(pool: &PgPool, username: &str) -> AppResult<TipSummary> {
    let summary = sqlx::query_as::<_, TipSummary>(
        r#"
        SELECT
            $1::TEXT AS creator_username,
            COUNT(*)::BIGINT AS total_tips,
            COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
            COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
            COALESCE(MAX(amount::NUMERIC), 0)::TEXT AS max_amount
        FROM tips
        WHERE creator_username = $1
        "#,
    )
    .bind(username)
    .fetch_one(pool)
    .await?;
    Ok(summary)
}

pub async fn get_creator_stats(state: &AppState, username: &str) -> AppResult<CreatorStats> {
    let cache_key = keys::creator_stats(username);

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        if let Some(stats) = redis_client::get::<CreatorStats>(&mut conn, &cache_key).await {
            return Ok(stats);
        }
    }

    if let Some(cache) = state.cache.as_ref() {
        match cache.get::<CreatorStats>(&cache_key).await {
            Ok(Some(stats)) => return Ok(stats),
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, key = %cache_key, "Creator stats cache read failed"),
        }
    }

    let pool = state.read_pool().await;
    let row = sqlx::query_as::<_, CreatorStatsRow>(
        r#"
        WITH creator_tips AS (
            SELECT
                id,
                amount::NUMERIC AS amount_xlm,
                transaction_hash,
                NULLIF(tipper_wallet, '') AS tipper_wallet,
                created_at
            FROM tips
            WHERE creator_username = $1
        ),
        summary AS (
            SELECT
                COALESCE(SUM(amount_xlm), 0)::TEXT AS total_amount_xlm,
                COUNT(*)::BIGINT AS tip_count,
                COUNT(DISTINCT tipper_wallet)::BIGINT AS unique_supporters
            FROM creator_tips
        ),
        ranked_supporters AS (
            SELECT
                tipper_wallet,
                SUM(amount_xlm) AS total_amount_xlm,
                COUNT(*)::BIGINT AS tip_count
            FROM creator_tips
            WHERE tipper_wallet IS NOT NULL
            GROUP BY tipper_wallet
            ORDER BY SUM(amount_xlm) DESC, COUNT(*) DESC, tipper_wallet ASC
            LIMIT 5
        ),
        recent_tips AS (
            SELECT
                id,
                amount_xlm,
                transaction_hash,
                tipper_wallet,
                created_at
            FROM creator_tips
            ORDER BY created_at DESC, id DESC
            LIMIT 20
        )
        SELECT
            $1::TEXT AS creator_username,
            summary.total_amount_xlm,
            summary.tip_count,
            summary.unique_supporters,
            COALESCE((
                SELECT jsonb_agg(
                    jsonb_build_object(
                        'tipper_wallet', tipper_wallet,
                        'total_amount_xlm', total_amount_xlm::TEXT,
                        'tip_count', tip_count
                    )
                    ORDER BY total_amount_xlm DESC, tip_count DESC, tipper_wallet ASC
                )
                FROM ranked_supporters
            ), '[]'::jsonb)::TEXT AS top_supporters,
            COALESCE((
                SELECT jsonb_agg(
                    jsonb_build_object(
                        'id', id::TEXT,
                        'amount_xlm', amount_xlm::TEXT,
                        'transaction_hash', transaction_hash,
                        'tipper_wallet', tipper_wallet,
                        'created_at', to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                    )
                    ORDER BY created_at DESC, id DESC
                )
                FROM recent_tips
            ), '[]'::jsonb)::TEXT AS tip_history
        FROM summary
        "#,
    )
    .bind(username)
    .fetch_one(&pool)
    .await?;

    let stats = row.into_stats()?;

    if let Some(conn) = state.redis.as_ref() {
        let mut conn = conn.clone();
        redis_client::set(&mut conn, &cache_key, &stats, redis_client::TTL_CREATOR).await;
    }

    if let Some(cache) = state.cache.as_ref() {
        if let Err(e) = cache
            .set(
                &cache_key,
                &stats,
                Duration::from_secs(CREATOR_STATS_TTL_SECS),
            )
            .await
        {
            tracing::warn!(error = %e, key = %cache_key, "Creator stats cache write failed");
        }
    }

    Ok(stats)
}

pub async fn get_daily_stats(
    pool: &PgPool,
    username: &str,
    query: &StatsQuery,
) -> AppResult<Vec<TipDailyStat>> {
    let days = query.clamped_days();
    let stats = sqlx::query_as::<_, TipDailyStat>(
        r#"
        SELECT
            creator_username,
            DATE(created_at) AS stat_date,
            COUNT(*)::BIGINT AS tip_count,
            COALESCE(SUM(amount::NUMERIC), 0)::TEXT AS total_amount,
            COALESCE(AVG(amount::NUMERIC), 0)::TEXT AS avg_amount,
            COALESCE(MAX(amount::NUMERIC), 0)::TEXT AS max_amount
        FROM tips
        WHERE creator_username = $1
          AND created_at >= NOW() - ($2 || ' days')::INTERVAL
        GROUP BY creator_username, DATE(created_at)
        ORDER BY stat_date DESC
        "#,
    )
    .bind(username)
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(stats)
}

/// Upsert aggregated daily stats into the tip_daily_stats table.
pub async fn aggregate_daily_stats(pool: &PgPool, username: &str) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO tip_daily_stats (creator_username, stat_date, tip_count, total_amount, avg_amount, max_amount)
        SELECT
            creator_username,
            DATE(created_at),
            COUNT(*),
            COALESCE(SUM(amount::NUMERIC), 0),
            COALESCE(AVG(amount::NUMERIC), 0),
            COALESCE(MAX(amount::NUMERIC), 0)
        FROM tips
        WHERE creator_username = $1
        GROUP BY creator_username, DATE(created_at)
        ON CONFLICT (creator_username, stat_date) DO UPDATE SET
            tip_count    = EXCLUDED.tip_count,
            total_amount = EXCLUDED.total_amount,
            avg_amount   = EXCLUDED.avg_amount,
            max_amount   = EXCLUDED.max_amount
        "#,
    )
    .bind(username)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_stats_row_decodes_json_aggregates() {
        let row = CreatorStatsRow {
            creator_username: "alice".to_string(),
            total_amount_xlm: "12.5".to_string(),
            tip_count: 2,
            unique_supporters: 1,
            top_supporters: r#"[{"tipper_wallet":"GABC","total_amount_xlm":"12.5","tip_count":2}]"#
                .to_string(),
            tip_history: r#"[{"id":"00000000-0000-0000-0000-000000000001","amount_xlm":"7.5","transaction_hash":"TX1","tipper_wallet":"GABC","created_at":"2026-07-04T00:00:00.000Z"}]"#
                .to_string(),
        };

        let stats = row.into_stats().expect("row decodes");

        assert_eq!(stats.creator_username, "alice");
        assert_eq!(stats.total_amount_xlm, "12.5");
        assert_eq!(stats.tip_count, 2);
        assert_eq!(stats.unique_supporters, 1);
        assert_eq!(stats.top_supporters[0].tipper_wallet, "GABC");
        assert_eq!(stats.tip_history[0].amount_xlm, "7.5");
    }
}
