use crate::errors::app_error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfo {
    pub shard_id: u32,
    pub min_key: u64,
    pub max_key: u64,
    pub connection_string: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ShardRouter {
    shards: Arc<HashMap<u32, ShardInfo>>,
    shard_count: u32,
}

impl ShardRouter {
    pub fn new(shard_count: u32) -> Self {
        Self {
            shards: Arc::new(HashMap::new()),
            shard_count,
        }
    }

    pub fn register_shard(&mut self, shard: ShardInfo) {
        Arc::get_mut(&mut self.shards)
            .unwrap()
            .insert(shard.shard_id, shard);
    }

    pub fn get_shard_id(&self, key: &str) -> u32 {
        let hash = Self::hash_key(key);
        (hash % self.shard_count as u64) as u32
    }

    fn hash_key(key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    pub fn get_shard(&self, shard_id: u32) -> Option<&ShardInfo> {
        self.shards.get(&shard_id)
    }

    pub fn get_all_shards(&self) -> Vec<&ShardInfo> {
        self.shards.values().collect()
    }
}

pub struct ShardingManager {
    router: ShardRouter,
    pools: HashMap<u32, PgPool>,
}

impl ShardingManager {
    pub fn new(router: ShardRouter) -> Self {
        Self {
            router,
            pools: HashMap::new(),
        }
    }

    pub async fn add_shard_pool(&mut self, shard_id: u32, pool: PgPool) {
        self.pools.insert(shard_id, pool);
    }

    pub fn get_pool_for_key(&self, key: &str) -> Result<&PgPool, AppError> {
        let shard_id = self.router.get_shard_id(key);
        self.pools
            .get(&shard_id)
            .ok_or_else(|| AppError::internal_error("Shard pool not found".to_string()))
    }

    pub async fn query_cross_shard(
        &self,
        query: &str,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let mut results = Vec::new();

        for pool in self.pools.values() {
            let rows: Vec<(serde_json::Value,)> = sqlx::query_as(query)
                .fetch_all(pool)
                .await
                .map_err(|e| AppError::database_error(e.to_string()))?;

            for (row,) in rows {
                results.push(row);
            }
        }

        Ok(results)
    }

    pub async fn rebalance_shards(&self) -> Result<(), AppError> {
        for pool in self.pools.values() {
            sqlx::query("ANALYZE")
                .execute(pool)
                .await
                .map_err(|e| AppError::database_error(e.to_string()))?;
        }

        Ok(())
    }

    pub async fn get_shard_stats(&self, shard_id: u32) -> Result<ShardStats, AppError> {
        let pool = self
            .pools
            .get(&shard_id)
            .ok_or_else(|| AppError::not_found("Shard not found".to_string()))?;

        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tips")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        let size_bytes: i64 = sqlx::query_scalar(
            "SELECT pg_total_relation_size('tips') as size",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        Ok(ShardStats {
            shard_id,
            row_count,
            size_bytes,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStats {
    pub shard_id: u32,
    pub row_count: i64,
    pub size_bytes: i64,
}
