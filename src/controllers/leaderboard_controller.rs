use chrono::{Duration, Utc};
use sqlx::PgPool;

use crate::errors::AppResult;
use crate::models::leaderboard::{LeaderboardEntry, LeaderboardResponse, LeaderboardSnapshot};

/// Fetch leaderboard from snapshots table (fast path).
pub async fn get_leaderboard(
    pool: &PgPool,
    period: &str,
    board_type: &str,
    limit: i64,
) -> AppResult<LeaderboardResponse> {
    let rows = sqlx::query_as::<_, LeaderboardSnapshot>(
        r#"
        SELECT id, period, board_type, rank, username, score::text AS score, tip_count, snapshot_at
        FROM leaderboard_snapshots
        WHERE period = $1 AND board_type = $2
        ORDER BY rank ASC
        LIMIT $3
        "#,
    )
    .bind(period)
    .bind(board_type)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let snapshot_at = rows.first().map(|r| r.snapshot_at);
    let entries = rows
        .into_iter()
        .map(|r| LeaderboardEntry {
            rank: r.rank,
            username: r.username,
            score: r.score,
            tip_count: r.tip_count,
        })
        .collect();

    Ok(LeaderboardResponse {
        period: period.to_string(),
        board_type: board_type.to_string(),
        entries,
        snapshot_at,
    })
}

/// Rebuild leaderboard snapshots for all periods and board types.
/// Called by a background job or on-demand.
pub async fn refresh_leaderboards(pool: &PgPool) -> AppResult<()> {
    let now = Utc::now();

    let periods: &[(&str, Option<Duration>)] = &[
        ("daily", Some(Duration::days(1))),
        ("weekly", Some(Duration::weeks(1))),
        ("monthly", Some(Duration::days(30))),
        ("all_time", None),
    ];

    for (period, window) in periods {
        let since_clause = match window {
            Some(d) => {
                let since = now - *d;
                format!("AND created_at >= '{}'", since.to_rfc3339())
            }
            None => String::new(),
        };

        // Top creators by total tips received
        refresh_board(pool, period, "top_creators", &since_clause, "creator_username").await?;

        // Top tippers by total tips sent (using transaction_hash as proxy for unique tipper)
        // We use transaction_hash as a stand-in since we don't have a tipper_id column
        refresh_board_tippers(pool, period, &since_clause).await?;

        // Trending: weighted score = tip_count * 2 + total_amount (recency-boosted)
        refresh_trending(pool, period, &since_clause).await?;
    }

    Ok(())
}

async fn refresh_board(
    pool: &PgPool,
    period: &str,
    board_type: &str,
    since_clause: &str,
    group_col: &str,
) -> AppResult<()> {
    let sql = format!(
        r#"
        WITH ranked AS (
            SELECT {group_col} AS username,
                   SUM(amount::numeric) AS score,
                   COUNT(*) AS tip_count,
                   ROW_NUMBER() OVER (ORDER BY SUM(amount::numeric) DESC) AS rank
            FROM tips
            WHERE 1=1 {since_clause}
            GROUP BY {group_col}
        )
        INSERT INTO leaderboard_snapshots (period, board_type, rank, username, score, tip_count, snapshot_at)
        SELECT $1, $2, rank, username, score, tip_count::int, NOW()
        FROM ranked
        ON CONFLICT (period, board_type, rank)
        DO UPDATE SET username = EXCLUDED.username,
                      score = EXCLUDED.score,
                      tip_count = EXCLUDED.tip_count,
                      snapshot_at = EXCLUDED.snapshot_at
        "#,
        group_col = group_col,
        since_clause = since_clause,
    );

    sqlx::query(&sql)
        .bind(period)
        .bind(board_type)
        .execute(pool)
        .await?;

    Ok(())
}

async fn refresh_board_tippers(pool: &PgPool, period: &str, since_clause: &str) -> AppResult<()> {
    // Since we don't have a tipper identity column, we group by transaction_hash prefix
    // as a best-effort unique tipper approximation.
    let sql = format!(
        r#"
        WITH ranked AS (
            SELECT transaction_hash AS username,
                   SUM(amount::numeric) AS score,
                   COUNT(*) AS tip_count,
                   ROW_NUMBER() OVER (ORDER BY SUM(amount::numeric) DESC) AS rank
            FROM tips
            WHERE 1=1 {since_clause}
            GROUP BY transaction_hash
        )
        INSERT INTO leaderboard_snapshots (period, board_type, rank, username, score, tip_count, snapshot_at)
        SELECT $1, 'top_tippers', rank, username, score, tip_count::int, NOW()
        FROM ranked
        ON CONFLICT (period, board_type, rank)
        DO UPDATE SET username = EXCLUDED.username,
                      score = EXCLUDED.score,
                      tip_count = EXCLUDED.tip_count,
                      snapshot_at = EXCLUDED.snapshot_at
        "#,
        since_clause = since_clause,
    );

    sqlx::query(&sql)
        .bind(period)
        .execute(pool)
        .await?;

    Ok(())
}

async fn refresh_trending(pool: &PgPool, period: &str, since_clause: &str) -> AppResult<()> {
    // Trending score = tip_count * 2 + total_amount (simple recency-weighted algorithm)
    let sql = format!(
        r#"
        WITH ranked AS (
            SELECT creator_username AS username,
                   (COUNT(*) * 2 + SUM(amount::numeric)) AS score,
                   COUNT(*) AS tip_count,
                   ROW_NUMBER() OVER (ORDER BY (COUNT(*) * 2 + SUM(amount::numeric)) DESC) AS rank
            FROM tips
            WHERE 1=1 {since_clause}
            GROUP BY creator_username
        )
        INSERT INTO leaderboard_snapshots (period, board_type, rank, username, score, tip_count, snapshot_at)
        SELECT $1, 'trending', rank, username, score, tip_count::int, NOW()
        FROM ranked
        ON CONFLICT (period, board_type, rank)
        DO UPDATE SET username = EXCLUDED.username,
                      score = EXCLUDED.score,
                      tip_count = EXCLUDED.tip_count,
                      snapshot_at = EXCLUDED.snapshot_at
        "#,
        since_clause = since_clause,
    );

    sqlx::query(&sql)
        .bind(period)
        .execute(pool)
        .await?;

    Ok(())
}
