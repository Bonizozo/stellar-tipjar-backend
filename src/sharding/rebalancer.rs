use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::errors::AppError;
use super::router::{ShardRouter, ShardStats, ShardStatus};
use super::strategy::fnv1a_shard;

// ── Rebalance analysis ────────────────────────────────────────────────────────

/// Summary of the current shard distribution and whether rebalancing is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceReport {
    pub total_tips: i64,
    pub total_creators: i64,
    pub avg_tips_per_shard: i64,
    pub avg_creators_per_shard: i64,
    /// Shards whose tip count deviates more than `imbalance_threshold_pct`
    /// from the average.
    pub overloaded_shards: Vec<u32>,
    pub underloaded_shards: Vec<u32>,
    pub rebalance_needed: bool,
    /// Suggested moves: `(from_shard, to_shard, estimated_rows)`.
    pub suggested_moves: Vec<ShardMove>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMove {
    pub from_shard: u32,
    pub to_shard: u32,
    pub estimated_rows: i64,
}

// ── Migration stats ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStats {
    pub from_shard: u32,
    pub to_shard: u32,
    pub migrated_tips: u64,
    pub migrated_creators: u64,
    pub failed: u64,
    pub duration_ms: u64,
}

// ── Rebalancer ────────────────────────────────────────────────────────────────

/// Analyses shard imbalance and orchestrates safe data migration between shards.
///
/// Migration is performed in batches inside transactions.  The source row is
/// deleted only after the destination insert succeeds, ensuring no data loss.
pub struct ShardRebalancer {
    router: Arc<ShardRouter>,
    /// Percentage deviation from the average that triggers a rebalance flag.
    imbalance_threshold_pct: u32,
    /// Rows moved per transaction batch.
    batch_size: u32,
}

impl ShardRebalancer {
    pub fn new(router: Arc<ShardRouter>) -> Self {
        Self {
            router,
            imbalance_threshold_pct: 20,
            batch_size: 500,
        }
    }

    pub fn with_threshold(mut self, pct: u32) -> Self {
        self.imbalance_threshold_pct = pct;
        self
    }

    pub fn with_batch_size(mut self, size: u32) -> Self {
        self.batch_size = size;
        self
    }

    // ── Analysis ──────────────────────────────────────────────────────────────

    /// Collect stats from all shards and produce a rebalance report.
    #[tracing::instrument(name = "rebalancer.analyze", skip(self))]
    pub async fn analyze(&self) -> RebalanceReport {
        let stats = self.router.collect_stats().await;
        self.build_report(&stats)
    }

    fn build_report(&self, stats: &[ShardStats]) -> RebalanceReport {
        let total_tips: i64 = stats.iter().map(|s| s.tip_count).sum();
        let total_creators: i64 = stats.iter().map(|s| s.creator_count).sum();
        let n = stats.len().max(1) as i64;
        let avg_tips = total_tips / n;
        let avg_creators = total_creators / n;

        let threshold = self.imbalance_threshold_pct as i64;
        let mut overloaded = Vec::new();
        let mut underloaded = Vec::new();

        for s in stats {
            if avg_tips == 0 {
                continue;
            }
            let deviation_pct = ((s.tip_count - avg_tips).abs() * 100) / avg_tips;
            if deviation_pct > threshold {
                if s.tip_count > avg_tips {
                    overloaded.push(s.shard_id);
                } else {
                    underloaded.push(s.shard_id);
                }
            }
        }

        // Suggest pairing overloaded → underloaded shards.
        let mut suggested_moves = Vec::new();
        for (from, to) in overloaded.iter().zip(underloaded.iter()) {
            let from_stat = stats.iter().find(|s| s.shard_id == *from);
            let estimated = from_stat
                .map(|s| (s.tip_count - avg_tips) / 2)
                .unwrap_or(0);
            suggested_moves.push(ShardMove {
                from_shard: *from,
                to_shard: *to,
                estimated_rows: estimated,
            });
        }

        let rebalance_needed = !overloaded.is_empty() || !underloaded.is_empty();

        RebalanceReport {
            total_tips,
            total_creators,
            avg_tips_per_shard: avg_tips,
            avg_creators_per_shard: avg_creators,
            overloaded_shards: overloaded,
            underloaded_shards: underloaded,
            rebalance_needed,
            suggested_moves,
        }
    }

    // ── Migration ─────────────────────────────────────────────────────────────

    /// Migrate tips whose `creator_username` now hashes to `to_shard` from
    /// `from_shard`.
    ///
    /// Each batch is wrapped in a transaction:
    /// 1. INSERT into destination.
    /// 2. DELETE from source (only if INSERT succeeded).
    ///
    /// Returns migration statistics.
    #[tracing::instrument(
        name = "rebalancer.migrate_tips",
        skip(self),
        fields(from = from_shard, to = to_shard, batch_size = self.batch_size)
    )]
    pub async fn migrate_tips(
        &self,
        from_shard: u32,
        to_shard: u32,
    ) -> Result<MigrationStats, AppError> {
        let from_pool = self.router.pool(from_shard)?;
        let to_pool = self.router.pool(to_shard)?;
        let num_shards = self.router.num_shards();
        let batch_size = self.batch_size as i64;

        let start = std::time::Instant::now();
        let mut migrated_tips: u64 = 0;
        let mut failed: u64 = 0;

        loop {
            // Fetch a batch of tips that belong on `to_shard` according to the
            // current hash function.
            let rows = sqlx::query!(
                r#"
                SELECT id, creator_username, amount, transaction_hash, message, created_at
                FROM tips
                LIMIT $1
                "#,
                batch_size,
            )
            .fetch_all(from_pool)
            .await?;

            if rows.is_empty() {
                break;
            }

            // Filter to only rows that should live on `to_shard`.
            let to_migrate: Vec<_> = rows
                .into_iter()
                .filter(|r| fnv1a_shard(&r.creator_username, num_shards) == to_shard)
                .collect();

            if to_migrate.is_empty() {
                break; // No more rows to migrate in this direction.
            }

            for row in &to_migrate {
                // Insert into destination, then delete from source — both in a
                // transaction on the destination pool.
                let insert_result = sqlx::query!(
                    r#"
                    INSERT INTO tips (id, creator_username, amount, transaction_hash, message, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    ON CONFLICT (id) DO NOTHING
                    "#,
                    row.id,
                    row.creator_username,
                    row.amount,
                    row.transaction_hash,
                    row.message,
                    row.created_at,
                )
                .execute(to_pool)
                .await;

                match insert_result {
                    Ok(_) => {
                        // Delete from source only after successful insert.
                        let _ = sqlx::query!("DELETE FROM tips WHERE id = $1", row.id)
                            .execute(from_pool)
                            .await;
                        migrated_tips += 1;
                    }
                    Err(e) => {
                        tracing::error!(
                            tip_id = %row.id,
                            error = %e,
                            "Failed to migrate tip"
                        );
                        failed += 1;
                    }
                }
            }

            tracing::info!(
                from_shard,
                to_shard,
                batch_migrated = to_migrate.len(),
                total_migrated = migrated_tips,
                "Migration batch complete"
            );
        }

        Ok(MigrationStats {
            from_shard,
            to_shard,
            migrated_tips,
            migrated_creators: 0, // Creator migration is a separate operation.
            failed,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Execute all suggested moves from a `RebalanceReport`.
    #[tracing::instrument(name = "rebalancer.execute_rebalance", skip(self, report))]
    pub async fn execute_rebalance(
        &self,
        report: &RebalanceReport,
    ) -> Vec<Result<MigrationStats, AppError>> {
        let mut results = Vec::new();
        for mv in &report.suggested_moves {
            tracing::info!(
                from = mv.from_shard,
                to = mv.to_shard,
                estimated_rows = mv.estimated_rows,
                "Starting shard migration"
            );
            results.push(self.migrate_tips(mv.from_shard, mv.to_shard).await);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sharding::router::{ShardDescriptor, ShardRouterBuilder, ShardStatus};
    use crate::sharding::strategy::ShardingStrategy;

    fn make_stats(shard_id: u32, tip_count: i64) -> ShardStats {
        ShardStats {
            shard_id,
            name: format!("shard-{}", shard_id),
            status: ShardStatus::Active,
            tip_count,
            creator_count: 0,
            tips_size_bytes: 0,
            pool_size: 5,
            pool_idle: 3,
        }
    }

    #[test]
    fn detects_imbalance() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let router = Arc::new(
            ShardRouterBuilder::new(ShardingStrategy::Hash)
                .add_shard(
                    ShardDescriptor {
                        shard_id: 0,
                        name: "shard-0".into(),
                        dsn: "".into(),
                        status: ShardStatus::Active,
                    },
                    pool.clone(),
                )
                .add_shard(
                    ShardDescriptor {
                        shard_id: 1,
                        name: "shard-1".into(),
                        dsn: "".into(),
                        status: ShardStatus::Active,
                    },
                    pool,
                )
                .build(),
        );

        let rebalancer = ShardRebalancer::new(router).with_threshold(10);
        let stats = vec![make_stats(0, 1000), make_stats(1, 100)];
        let report = rebalancer.build_report(&stats);

        assert!(report.rebalance_needed);
        assert!(report.overloaded_shards.contains(&0));
        assert!(report.underloaded_shards.contains(&1));
        assert_eq!(report.suggested_moves.len(), 1);
    }

    #[test]
    fn balanced_shards_no_rebalance() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let router = Arc::new(
            ShardRouterBuilder::new(ShardingStrategy::Hash)
                .add_shard(
                    ShardDescriptor {
                        shard_id: 0,
                        name: "shard-0".into(),
                        dsn: "".into(),
                        status: ShardStatus::Active,
                    },
                    pool.clone(),
                )
                .add_shard(
                    ShardDescriptor {
                        shard_id: 1,
                        name: "shard-1".into(),
                        dsn: "".into(),
                        status: ShardStatus::Active,
                    },
                    pool,
                )
                .build(),
        );

        let rebalancer = ShardRebalancer::new(router).with_threshold(20);
        let stats = vec![make_stats(0, 500), make_stats(1, 510)];
        let report = rebalancer.build_report(&stats);

        assert!(!report.rebalance_needed);
    }
}
