use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;
use crate::sharding::monitor::{record_cross_query, record_query, ShardClusterHealth, ShardMonitor};
use crate::sharding::rebalancer::{RebalanceReport, ShardRebalancer};
use crate::sharding::router::{ShardDescriptor, ShardRouter, ShardRouterBuilder, ShardStats, ShardStatus};
use crate::sharding::strategy::ShardingStrategy;

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for the sharding subsystem, loaded from environment variables.
///
/// | Variable                  | Default       | Description                          |
/// |---------------------------|---------------|--------------------------------------|
/// | `SHARD_COUNT`             | `1`           | Number of shards                     |
/// | `SHARD_STRATEGY`          | `hash`        | `hash` \| `range` \| `directory`     |
/// | `SHARD_{n}_DSN`           | —             | DSN for shard n (0-indexed)          |
/// | `SHARD_MAX_CONNECTIONS`   | `10`          | Max pool connections per shard       |
/// | `SHARD_MIN_CONNECTIONS`   | `2`           | Min pool connections per shard       |
#[derive(Debug, Clone)]
pub struct ShardingConfig {
    pub num_shards: u32,
    pub strategy: ShardingStrategy,
    pub shard_dsns: Vec<String>,
    pub max_connections: u32,
    pub min_connections: u32,
}

impl ShardingConfig {
    pub fn from_env(fallback_dsn: &str) -> Self {
        let num_shards: u32 = std::env::var("SHARD_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        let strategy = match std::env::var("SHARD_STRATEGY")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "range" => ShardingStrategy::Range,
            "directory" => ShardingStrategy::Directory,
            _ => ShardingStrategy::Hash,
        };

        let shard_dsns: Vec<String> = (0..num_shards)
            .map(|i| {
                std::env::var(format!("SHARD_{}_DSN", i))
                    .unwrap_or_else(|_| fallback_dsn.to_string())
            })
            .collect();

        let max_connections: u32 = std::env::var("SHARD_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let min_connections: u32 = std::env::var("SHARD_MIN_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);

        Self {
            num_shards,
            strategy,
            shard_dsns,
            max_connections,
            min_connections,
        }
    }
}

// ── Manager ───────────────────────────────────────────────────────────────────

/// Top-level sharding manager.
///
/// Holds the `ShardRouter`, `ShardRebalancer`, and `ShardMonitor` and exposes
/// the high-level API used by the rest of the application.
///
/// Designed to be held in an `Arc` inside `AppState`.
pub struct ShardingManager {
    pub router: Arc<ShardRouter>,
    pub rebalancer: Arc<ShardRebalancer>,
    pub monitor: Arc<ShardMonitor>,
}

impl ShardingManager {
    /// Build a `ShardingManager` from a `ShardingConfig`.
    ///
    /// Opens a connection pool for each shard.  If a shard DSN is unavailable
    /// the pool is created lazily (no immediate connection attempt) so the app
    /// can start even when some shards are temporarily offline.
    pub async fn from_config(config: &ShardingConfig) -> anyhow::Result<Self> {
        let mut builder = ShardRouterBuilder::new(config.strategy);

        for (i, dsn) in config.shard_dsns.iter().enumerate() {
            let shard_id = i as u32;

            let pool = PgPoolOptions::new()
                .max_connections(config.max_connections)
                .min_connections(config.min_connections)
                .connect_lazy(dsn)?;

            let descriptor = ShardDescriptor {
                shard_id,
                name: format!("shard-{}", shard_id),
                dsn: dsn.clone(),
                status: ShardStatus::Active,
            };

            builder = builder.add_shard(descriptor, pool);
        }

        let router = Arc::new(builder.build());
        let rebalancer = Arc::new(ShardRebalancer::new(Arc::clone(&router)));
        let monitor = Arc::new(ShardMonitor::new(Arc::clone(&router)));

        tracing::info!(
            num_shards = config.num_shards,
            strategy   = ?config.strategy,
            "Sharding manager initialised"
        );

        Ok(Self {
            router,
            rebalancer,
            monitor,
        })
    }

    /// Start the background monitoring task.
    pub fn start_monitor(&self) {
        Arc::clone(&self.monitor).spawn();
    }

    // ── Routing helpers ───────────────────────────────────────────────────────

    /// Get the pool for the shard that owns `creator_username`.
    pub fn pool_for_creator(&self, username: &str) -> Result<&PgPool, AppError> {
        let shard_id = self.router.shard_for_username(username);
        record_query(shard_id, "route");
        self.router.pool(shard_id)
    }

    /// Get the pool for the shard that owns `tip_id` (UUID-based routing).
    pub fn pool_for_tip(&self, tip_id: Uuid) -> Result<&PgPool, AppError> {
        let shard_id = self.router.shard_for_uuid(tip_id);
        record_query(shard_id, "route");
        self.router.pool(shard_id)
    }

    // ── Cross-shard queries ───────────────────────────────────────────────────

    /// Count total tips across all shards.
    #[tracing::instrument(name = "sharding.count_tips_all_shards", skip(self))]
    pub async fn count_tips_all_shards(&self) -> Result<i64, AppError> {
        record_cross_query("count_tips");
        let results = self
            .router
            .fan_out(|shard_id, pool| async move {
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tips")
                    .fetch_one(pool)
                    .await
                    .map_err(AppError::from)?;
                Ok(count)
            })
            .await?;

        Ok(results.iter().map(|(_, c)| c).sum())
    }

    /// Count total creators across all shards.
    #[tracing::instrument(name = "sharding.count_creators_all_shards", skip(self))]
    pub async fn count_creators_all_shards(&self) -> Result<i64, AppError> {
        record_cross_query("count_creators");
        let results = self
            .router
            .fan_out(|shard_id, pool| async move {
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM creators")
                    .fetch_one(pool)
                    .await
                    .map_err(AppError::from)?;
                Ok(count)
            })
            .await?;

        Ok(results.iter().map(|(_, c)| c).sum())
    }

    // ── Monitoring ────────────────────────────────────────────────────────────

    /// Collect per-shard statistics.
    pub async fn stats(&self) -> Vec<ShardStats> {
        self.router.collect_stats().await
    }

    /// Full cluster health check.
    pub async fn health(&self) -> ShardClusterHealth {
        self.monitor.health_check().await
    }

    // ── Rebalancing ───────────────────────────────────────────────────────────

    /// Analyse the current shard distribution.
    pub async fn analyze_balance(&self) -> RebalanceReport {
        self.rebalancer.analyze().await
    }
}

// ── Singleton initialisation ──────────────────────────────────────────────────

/// Initialise the sharding manager from environment variables.
///
/// Returns `None` when `SHARD_COUNT` is 1 (or unset) and no explicit shard
/// DSNs are configured — in that case the application uses the single primary
/// pool directly and sharding is a no-op.
pub async fn init_sharding(primary_dsn: &str) -> Option<Arc<ShardingManager>> {
    let config = ShardingConfig::from_env(primary_dsn);

    if config.num_shards <= 1
        && std::env::var("SHARD_0_DSN").is_err()
        && std::env::var("SHARD_1_DSN").is_err()
    {
        tracing::info!("Sharding disabled (SHARD_COUNT=1 and no SHARD_n_DSN configured)");
        return None;
    }

    match ShardingManager::from_config(&config).await {
        Ok(manager) => {
            manager.start_monitor();
            tracing::info!(
                num_shards = config.num_shards,
                "Sharding manager started"
            );
            Some(Arc::new(manager))
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to initialise sharding manager — running without sharding");
            None
        }
    }
}
