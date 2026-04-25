use sqlx::PgPool;
use uuid::Uuid;
use crate::errors::AppError;
use super::strategy::ShardKey;

pub struct ShardRouter {
    shards: Vec<PgPool>,
    num_shards: u32,
}

impl ShardRouter {
    pub fn new(shards: Vec<PgPool>) -> Self {
        let num_shards = shards.len() as u32;
        Self { shards, num_shards }
    }

    pub fn get_shard_id(&self, key: &ShardKey) -> u32 {
        key.shard_id % self.num_shards
    }

    pub fn get_shard_pool(&self, key: &ShardKey) -> &PgPool {
        let shard_id = self.get_shard_id(key);
        &self.shards[shard_id as usize]
    }

    pub async fn get_shard_stats(&self) -> Result<Vec<ShardStats>, AppError> {
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
}

#[derive(Debug, Clone)]
pub struct ShardStats {
    pub shard_id: u32,
    pub creator_count: i64,
}
