use sqlx::PgPool;
use crate::errors::AppError;
use super::router::ShardStats;

pub struct ShardRebalancer {
    shards: Vec<PgPool>,
}

impl ShardRebalancer {
    pub fn new(shards: Vec<PgPool>) -> Self {
        Self { shards }
    }

    pub async fn analyze_imbalance(&self) -> Result<Vec<ShardStats>, AppError> {
        let mut stats = Vec::new();
        
        for (i, shard) in self.shards.iter().enumerate() {
            let row = sqlx::query!(
                "SELECT COUNT(*) as creator_count FROM creators"
            )
            .fetch_one(shard)
            .await?;

            stats.push(ShardStats {
                shard_id: i as u32,
                creator_count: row.creator_count.unwrap_or(0),
            });
        }
        
        Ok(stats)
    }

    pub async fn rebalance_shards(&self, stats: &[ShardStats]) -> Result<RebalanceReport, AppError> {
        let total_creators: i64 = stats.iter().map(|s| s.creator_count).sum();
        let avg_per_shard = total_creators / stats.len() as i64;
        
        let mut imbalanced_shards = Vec::new();
        for stat in stats {
            let deviation = (stat.creator_count - avg_per_shard).abs();
            if deviation > avg_per_shard / 10 {
                imbalanced_shards.push(stat.shard_id);
            }
        }

        Ok(RebalanceReport {
            total_creators,
            avg_per_shard,
            imbalanced_shards,
            rebalance_needed: !imbalanced_shards.is_empty(),
        })
    }

    pub async fn migrate_data(
        &self,
        from_shard: u32,
        to_shard: u32,
        batch_size: i32,
    ) -> Result<MigrationStats, AppError> {
        let from_pool = &self.shards[from_shard as usize];
        let to_pool = &self.shards[to_shard as usize];

        let mut migrated = 0;
        let mut failed = 0;

        // Fetch batch of creators from source shard
        let creators = sqlx::query!(
            "SELECT id, username, wallet_address FROM creators LIMIT $1",
            batch_size as i64
        )
        .fetch_all(from_pool)
        .await?;

        for creator in creators {
            match sqlx::query(
                "INSERT INTO creators (id, username, wallet_address, created_at) 
                 VALUES ($1, $2, $3, NOW())"
            )
            .bind(creator.id)
            .bind(&creator.username)
            .bind(&creator.wallet_address)
            .execute(to_pool)
            .await {
                Ok(_) => migrated += 1,
                Err(_) => failed += 1,
            }
        }

        Ok(MigrationStats {
            migrated,
            failed,
            total: migrated + failed,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RebalanceReport {
    pub total_creators: i64,
    pub avg_per_shard: i64,
    pub imbalanced_shards: Vec<u32>,
    pub rebalance_needed: bool,
}

#[derive(Debug, Clone)]
pub struct MigrationStats {
    pub migrated: i32,
    pub failed: i32,
    pub total: i32,
}
