use std::collections::HashMap;
use std::sync::Arc;

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::errors::AppError;
use super::strategy::{fnv1a_shard, ShardKey, ShardingStrategy};

// ── Shard descriptor ──────────────────────────────────────────────────────────

/// Static metadata about a single shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardDescriptor {
    pub shard_id: u32,
    /// Human-readable label, e.g. `"shard-0"`.
    pub name: String,
    /// DSN used to connect to this shard's Postgres instance.
    pub dsn: String,
    /// Current operational status.
    pub status: ShardStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShardStatus {
    Active,
    ReadOnly,
    Draining,
    Offline,
}

// ── Per-shard runtime stats ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStats {
    pub shard_id: u32,
    pub name: String,
    pub status: ShardStatus,
    /// Number of rows in the `tips` table on this shard.
    pub tip_count: i64,
    /// Number of rows in the `creators` table on this shard.
    pub creator_count: i64,
    /// Approximate on-disk size of the `tips` table in bytes.
    pub tips_size_bytes: i64,
    /// Active + idle connections in the pool.
    pub pool_size: u32,
    /// Idle connections in the pool.
    pub pool_idle: u32,
}

// ── Shard router ─────────────────────────────────────────────────────────────

/// Routes logical keys to physical shard connection pools.
///
/// The router is `Clone + Send + Sync` and is designed to be held in an
/// `Arc` inside `AppState`.
#[derive(Clone)]
pub struct ShardRouter {
    /// Ordered list of shard descriptors (index == shard_id).
    descriptors: Arc<Vec<ShardDescriptor>>,
    /// Connection pool per shard_id.
    pools: Arc<HashMap<u32, PgPool>>,
    /// Number of shards (cached for hot-path arithmetic).
    num_shards: u32,
    /// Routing algorithm.
    strategy: ShardingStrategy,
}

impl ShardRouter {
    /// Build a router from a pre-constructed map of shard_id → pool.
    pub fn new(
        descriptors: Vec<ShardDescriptor>,
        pools: HashMap<u32, PgPool>,
        strategy: ShardingStrategy,
    ) -> Self {
        let num_shards = descriptors.len() as u32;
        Self {
            descriptors: Arc::new(descriptors),
            pools: Arc::new(pools),
            num_shards,
            strategy,
        }
    }

    /// Number of configured shards.
    pub fn num_shards(&self) -> u32 {
        self.num_shards
    }

    // ── Key resolution ────────────────────────────────────────────────────────

    /// Resolve a creator username to a shard id.
    pub fn shard_for_username(&self, username: &str) -> u32 {
        fnv1a_shard(username, self.num_shards)
    }

    /// Resolve a UUID (creator_id, tip_id) to a shard id.
    pub fn shard_for_uuid(&self, id: Uuid) -> u32 {
        ShardKey::from_uuid(id, self.num_shards).shard_id
    }

    /// Resolve an arbitrary string key to a shard id.
    pub fn shard_for_key(&self, key: &str) -> u32 {
        fnv1a_shard(key, self.num_shards)
    }

    // ── Pool access ───────────────────────────────────────────────────────────

    /// Get the connection pool for a specific shard id.
    pub fn pool(&self, shard_id: u32) -> Result<&PgPool, AppError> {
        self.pools.get(&shard_id).ok_or_else(|| {
            AppError::service_unavailable(format!("Shard {} pool not found", shard_id))
        })
    }

    /// Get the pool for the shard that owns `username`.
    pub fn pool_for_username(&self, username: &str) -> Result<&PgPool, AppError> {
        self.pool(self.shard_for_username(username))
    }

    /// Get the pool for the shard that owns `id`.
    pub fn pool_for_uuid(&self, id: Uuid) -> Result<&PgPool, AppError> {
        self.pool(self.shard_for_uuid(id))
    }

    /// Iterator over all active shard pools.
    pub fn all_pools(&self) -> impl Iterator<Item = (u32, &PgPool)> {
        self.pools.iter().map(|(id, pool)| (*id, pool))
    }

    /// All shard descriptors.
    pub fn descriptors(&self) -> &[ShardDescriptor] {
        &self.descriptors
    }

    // ── Cross-shard fan-out ───────────────────────────────────────────────────

    /// Execute an async closure on every shard in parallel and collect results.
    ///
    /// Results are returned in shard_id order.  Errors from individual shards
    /// are collected; if any shard fails the entire call returns an error
    /// containing the first failure.
    pub async fn fan_out<F, Fut, T>(&self, f: F) -> Result<Vec<(u32, T)>, AppError>
    where
        F: Fn(u32, &PgPool) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, AppError>> + Send,
        T: Send,
    {
        let mut futures = Vec::with_capacity(self.num_shards as usize);

        for (shard_id, pool) in self.all_pools() {
            futures.push(async move {
                let result = f(shard_id, pool).await?;
                Ok::<(u32, T), AppError>((shard_id, result))
            });
        }

        let results = join_all(futures).await;

        let mut output = Vec::with_capacity(results.len());
        for result in results {
            output.push(result?);
        }

        // Sort by shard_id for deterministic ordering.
        output.sort_by_key(|(id, _)| *id);
        Ok(output)
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    /// Collect per-shard statistics from all active shards in parallel.
    #[tracing::instrument(name = "shard_router.collect_stats", skip(self))]
    pub async fn collect_stats(&self) -> Vec<ShardStats> {
        let descriptors = Arc::clone(&self.descriptors);
        let pools = Arc::clone(&self.pools);

        let mut futures = Vec::new();
        for desc in descriptors.iter() {
            let desc = desc.clone();
            let pool = pools.get(&desc.shard_id).cloned();
            futures.push(async move {
                let pool = match pool {
                    Some(p) => p,
                    None => {
                        return ShardStats {
                            shard_id: desc.shard_id,
                            name: desc.name.clone(),
                            status: ShardStatus::Offline,
                            tip_count: 0,
                            creator_count: 0,
                            tips_size_bytes: 0,
                            pool_size: 0,
                            pool_idle: 0,
                        }
                    }
                };

                let tip_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tips")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(0);

                let creator_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM creators")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(0);

                let tips_size_bytes: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(pg_total_relation_size('tips'), 0)",
                )
                .fetch_one(&pool)
                .await
                .unwrap_or(0);

                ShardStats {
                    shard_id: desc.shard_id,
                    name: desc.name.clone(),
                    status: desc.status,
                    tip_count,
                    creator_count,
                    tips_size_bytes,
                    pool_size: pool.size(),
                    pool_idle: pool.num_idle() as u32,
                }
            });
        }

        join_all(futures).await
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Convenience builder for constructing a `ShardRouter` from environment
/// variables or explicit configuration.
pub struct ShardRouterBuilder {
    descriptors: Vec<ShardDescriptor>,
    pools: HashMap<u32, PgPool>,
    strategy: ShardingStrategy,
}

impl ShardRouterBuilder {
    pub fn new(strategy: ShardingStrategy) -> Self {
        Self {
            descriptors: Vec::new(),
            pools: HashMap::new(),
            strategy,
        }
    }

    pub fn add_shard(mut self, descriptor: ShardDescriptor, pool: PgPool) -> Self {
        self.pools.insert(descriptor.shard_id, pool);
        self.descriptors.push(descriptor);
        self
    }

    pub fn build(mut self) -> ShardRouter {
        // Sort descriptors by shard_id for deterministic ordering.
        self.descriptors.sort_by_key(|d| d.shard_id);
        ShardRouter::new(self.descriptors, self.pools, self.strategy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_for_username_is_stable() {
        // Build a minimal router with lazy pools (no real DB needed).
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let router = ShardRouterBuilder::new(ShardingStrategy::Hash)
            .add_shard(
                ShardDescriptor {
                    shard_id: 0,
                    name: "shard-0".into(),
                    dsn: "postgres://localhost/shard0".into(),
                    status: ShardStatus::Active,
                },
                pool.clone(),
            )
            .add_shard(
                ShardDescriptor {
                    shard_id: 1,
                    name: "shard-1".into(),
                    dsn: "postgres://localhost/shard1".into(),
                    status: ShardStatus::Active,
                },
                pool,
            )
            .build();

        let s1 = router.shard_for_username("alice");
        let s2 = router.shard_for_username("alice");
        assert_eq!(s1, s2);
        assert!(s1 < 2);
    }

    #[test]
    fn pool_returns_error_for_unknown_shard() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let router = ShardRouterBuilder::new(ShardingStrategy::Hash)
            .add_shard(
                ShardDescriptor {
                    shard_id: 0,
                    name: "shard-0".into(),
                    dsn: "postgres://localhost/shard0".into(),
                    status: ShardStatus::Active,
                },
                pool,
            )
            .build();

        assert!(router.pool(99).is_err());
    }
}
