use sqlx::PgPool;
use chrono::{Utc, Duration};
use std::env;
use tracing::{info, warn};

use crate::models::creator::Creator;

/// Generate daily tip summary
pub async fn generate_daily_summary(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let yesterday = Utc::now() - Duration::days(1);
    
    let summary = sqlx::query!(
        r#"
        INSERT INTO daily_summaries (date, total_tips, total_amount, unique_creators, unique_tippers)
        SELECT 
            DATE($1) as date,
            COUNT(*) as total_tips,
            SUM(amount) as total_amount,
            COUNT(DISTINCT creator_id) as unique_creators,
            COUNT(DISTINCT tipper_address) as unique_tippers
        FROM tips
        WHERE created_at >= $1 AND created_at < $1 + INTERVAL '1 day'
        ON CONFLICT (date) DO UPDATE SET
            total_tips = EXCLUDED.total_tips,
            total_amount = EXCLUDED.total_amount,
            unique_creators = EXCLUDED.unique_creators,
            unique_tippers = EXCLUDED.unique_tippers
        "#,
        yesterday.naive_utc()
    )
    .execute(pool)
    .await?;

    info!("Daily summary generated: {} rows affected", summary.rows_affected());
    Ok(())
}

/// Generate weekly creator reports
pub async fn generate_weekly_report(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let last_week = Utc::now() - Duration::weeks(1);
    
    let reports = sqlx::query!(
        r#"
        SELECT 
            c.id,
            c.username,
            c.email,
            COUNT(t.id) as tip_count,
            COALESCE(SUM(t.amount), 0) as total_amount
        FROM creators c
        LEFT JOIN tips t ON c.id = t.creator_id 
            AND t.created_at >= $1
        WHERE c.email IS NOT NULL
        GROUP BY c.id, c.username, c.email
        HAVING COUNT(t.id) > 0
        "#,
        last_week.naive_utc()
    )
    .fetch_all(pool)
    .await?;

    info!("Generated {} weekly reports", reports.len());
    
    // TODO: Send email notifications with reports
    for report in reports {
        info!(
            "Creator {} received {} tips totaling {} in the last week",
            report.username, report.tip_count.unwrap_or(0), report.total_amount.unwrap_or(rust_decimal::Decimal::ZERO)
        );
    }

    Ok(())
}

/// Cleanup old data (logs, expired sessions, etc.)
pub async fn cleanup_old_data(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let cutoff_date = Utc::now() - Duration::days(90);
    
    // Clean up old tip logs
    let deleted_logs = sqlx::query!(
        "DELETE FROM tip_logs WHERE created_at < $1",
        cutoff_date.naive_utc()
    )
    .execute(pool)
    .await?;

    info!("Deleted {} old tip logs", deleted_logs.rows_affected());

    // Clean up old events
    let deleted_events = sqlx::query!(
        "DELETE FROM events WHERE created_at < $1 AND processed = true",
        cutoff_date.naive_utc()
    )
    .execute(pool)
    .await?;

    info!("Deleted {} old events", deleted_events.rows_affected());

    // Vacuum analyze to reclaim space
    sqlx::query("VACUUM ANALYZE tips, tip_logs, events")
        .execute(pool)
        .await?;

    info!("Database vacuum completed");
    Ok(())
}

/// Warm cache with frequently accessed data using the multi-layer cache.
pub async fn warm_cache(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let cache = std::sync::Arc::new(crate::cache::MultiLayerCache::with_defaults());
    let warm_source = std::sync::Arc::new(crate::cache::CreatorWarmSource { pool: pool.clone() });
    let warmer = crate::cache::CacheWarmer::new(
        std::sync::Arc::clone(&cache),
        warm_source,
        std::time::Duration::from_secs(300),
    );

    let count = warmer.warm_popular(100).await?;
    info!("Warmed cache with {} top creators", count);

    // Warm leaderboard snapshots
    let board_types = ["top_creators", "top_tippers", "trending"];
    let periods = ["daily", "weekly", "monthly", "all_time"];
    let cache_ttl = std::time::Duration::from_secs(300);

    for board_type in &board_types {
        for period in &periods {
            let response = crate::controllers::leaderboard_controller::get_leaderboard(
                pool,
                period,
                board_type,
                50,
            )
            .await;
            if let Ok(result) = response {
                let key = crate::cache::keys::leaderboard(board_type, period, 50);
                let _ = cache.set(&key, &result, cache_ttl).await;
            }
        }
    }

    Ok(())
}

/// Aggregate analytics data
pub async fn aggregate_analytics(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let hour_ago = now - Duration::hours(1);
    
    // Aggregate hourly stats
    let stats = sqlx::query!(
        r#"
        INSERT INTO hourly_analytics (hour, total_tips, total_amount, avg_amount)
        SELECT 
            DATE_TRUNC('hour', $1) as hour,
            COUNT(*) as total_tips,
            SUM(amount) as total_amount,
            AVG(amount) as avg_amount
        FROM tips
        WHERE created_at >= $2 AND created_at < $1
        ON CONFLICT (hour) DO UPDATE SET
            total_tips = EXCLUDED.total_tips,
            total_amount = EXCLUDED.total_amount,
            avg_amount = EXCLUDED.avg_amount
        "#,
        now.naive_utc(),
        hour_ago.naive_utc()
    )
    .execute(pool)
    .await?;

    info!("Aggregated analytics: {} rows affected", stats.rows_affected());
    Ok(())
}
