use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::router::{ShardRouter, ShardStats, ShardStatus};

// ── Prometheus metrics ────────────────────────────────────────────────────────

use lazy_static::lazy_static;
use prometheus::{
    register_gauge_vec, register_histogram_vec, register_int_counter_vec, GaugeVec,
    HistogramVec, IntCounterVec,
};

lazy_static! {
    /// Number of rows in the `tips` table per shard.
    pub static ref SHARD_TIP_COUNT: GaugeVec = register_gauge_vec!(
        "shard_tip_count",
        "Number of tips rows per shard",
        &["shard_id", "shard_name"]
    )
    .unwrap();

    /// Number of rows in the `creators` table per shard.
    pub static ref SHARD_CREATOR_COUNT: GaugeVec = register_gauge_vec!(
        "shard_creator_count",
        "Number of creator rows per shard",
        &["shard_id", "shard_name"]
    )
    .unwrap();

    /// On-disk size of the `tips` table per shard (bytes).
    pub static ref SHARD_SIZE_BYTES: GaugeVec = register_gauge_vec!(
        "shard_size_bytes",
        "Approximate on-disk size of the tips table per shard",
        &["shard_id", "shard_name"]
    )
    .unwrap();

    /// Connection pool size per shard.
    pub static ref SHARD_POOL_SIZE: GaugeVec = register_gauge_vec!(
        "shard_pool_size",
        "Connection pool size per shard",
        &["shard_id", "shard_name", "state"]
    )
    .unwrap();

    /// Total queries routed to each shard.
    pub static ref SHARD_QUERIES_TOTAL: IntCounterVec = register_int_counter_vec!(
        "shard_queries_total",
        "Total queries routed to each shard",
        &["shard_id", "operation"]
    )
    .unwrap();

    /// Query latency per shard.
    pub static ref SHARD_QUERY_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "shard_query_duration_seconds",
        "Query duration per shard",
        &["shard_id", "operation"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
    )
    .unwrap();

    /// Number of cross-shard fan-out queries.
    pub static ref SHARD_CROSS_QUERIES_TOTAL: IntCounterVec = register_int_counter_vec!(
        "shard_cross_queries_total",
        "Total cross-shard fan-out queries",
        &["operation"]
    )
    .unwrap();

    /// Rebalance operations triggered.
    pub static ref SHARD_REBALANCE_TOTAL: IntCounterVec = register_int_counter_vec!(
        "shard_rebalance_total",
        "Total shard rebalance operations",
        &["status"]  // "started" | "completed" | "failed"
    )
    .unwrap();
}

// ── Health snapshot ───────────────────────────────────────────────────────────

/// Health status of a single shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardHealth {
    pub shard_id: u32,
    pub name: String,
    pub status: ShardStatus,
    pub reachable: bool,
    /// Round-trip time for a `SELECT 1` ping in milliseconds.
    pub ping_ms: Option<u64>,
    pub tip_count: i64,
    pub creator_count: i64,
    pub tips_size_bytes: i64,
    pub pool_size: u32,
    pub pool_idle: u32,
}

/// Aggregated health across all shards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardClusterHealth {
    pub total_shards: u32,
    pub healthy_shards: u32,
    pub degraded_shards: u32,
    pub offline_shards: u32,
    pub total_tips: i64,
    pub total_creators: i64,
    pub total_size_bytes: i64,
    pub shards: Vec<ShardHealth>,
}

// ── Monitor ───────────────────────────────────────────────────────────────────

/// Periodically collects shard statistics and exports them to Prometheus.
pub struct ShardMonitor {
    router: Arc<ShardRouter>,
    /// How often to scrape shard stats.
    scrape_interval: Duration,
}

impl ShardMonitor {
    pub fn new(router: Arc<ShardRouter>) -> Self {
        Self {
            router,
            scrape_interval: Duration::from_secs(30),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.scrape_interval = interval;
        self
    }

    /// Spawn a background task that scrapes shard stats on `scrape_interval`.
    pub fn spawn(self: Arc<Self>) {
        let monitor = Arc::clone(&self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(monitor.scrape_interval);
            loop {
                interval.tick().await;
                monitor.scrape().await;
            }
        });
    }

    /// Collect stats from all shards and update Prometheus gauges.
    #[tracing::instrument(name = "shard_monitor.scrape", skip(self))]
    pub async fn scrape(&self) {
        let stats = self.router.collect_stats().await;
        for s in &stats {
            let sid = s.shard_id.to_string();
            let name = &s.name;

            SHARD_TIP_COUNT
                .with_label_values(&[&sid, name])
                .set(s.tip_count as f64);

            SHARD_CREATOR_COUNT
                .with_label_values(&[&sid, name])
                .set(s.creator_count as f64);

            SHARD_SIZE_BYTES
                .with_label_values(&[&sid, name])
                .set(s.tips_size_bytes as f64);

            SHARD_POOL_SIZE
                .with_label_values(&[&sid, name, "total"])
                .set(s.pool_size as f64);

            SHARD_POOL_SIZE
                .with_label_values(&[&sid, name, "idle"])
                .set(s.pool_idle as f64);
        }

        tracing::debug!(shards = stats.len(), "Shard metrics scraped");
    }

    /// Perform a health check across all shards.
    #[tracing::instrument(name = "shard_monitor.health_check", skip(self))]
    pub async fn health_check(&self) -> ShardClusterHealth {
        let stats = self.router.collect_stats().await;
        let mut shard_healths = Vec::with_capacity(stats.len());

        for s in &stats {
            let (reachable, ping_ms) = self.ping_shard(s.shard_id).await;

            shard_healths.push(ShardHealth {
                shard_id: s.shard_id,
                name: s.name.clone(),
                status: s.status,
                reachable,
                ping_ms,
                tip_count: s.tip_count,
                creator_count: s.creator_count,
                tips_size_bytes: s.tips_size_bytes,
                pool_size: s.pool_size,
                pool_idle: s.pool_idle,
            });
        }

        let total = shard_healths.len() as u32;
        let healthy = shard_healths
            .iter()
            .filter(|h| h.reachable && h.status == ShardStatus::Active)
            .count() as u32;
        let offline = shard_healths
            .iter()
            .filter(|h| !h.reachable || h.status == ShardStatus::Offline)
            .count() as u32;
        let degraded = total - healthy - offline;

        let total_tips: i64 = shard_healths.iter().map(|h| h.tip_count).sum();
        let total_creators: i64 = shard_healths.iter().map(|h| h.creator_count).sum();
        let total_size: i64 = shard_healths.iter().map(|h| h.tips_size_bytes).sum();

        ShardClusterHealth {
            total_shards: total,
            healthy_shards: healthy,
            degraded_shards: degraded,
            offline_shards: offline,
            total_tips,
            total_creators,
            total_size_bytes: total_size,
            shards: shard_healths,
        }
    }

    /// Ping a shard with `SELECT 1` and return (reachable, latency_ms).
    async fn ping_shard(&self, shard_id: u32) -> (bool, Option<u64>) {
        let pool = match self.router.pool(shard_id) {
            Ok(p) => p,
            Err(_) => return (false, None),
        };

        let start = std::time::Instant::now();
        match sqlx::query("SELECT 1").execute(pool).await {
            Ok(_) => (true, Some(start.elapsed().as_millis() as u64)),
            Err(_) => (false, None),
        }
    }
}

/// Record a query routed to a shard (call from the data access layer).
pub fn record_query(shard_id: u32, operation: &str) {
    SHARD_QUERIES_TOTAL
        .with_label_values(&[&shard_id.to_string(), operation])
        .inc();
}

/// Record a cross-shard fan-out query.
pub fn record_cross_query(operation: &str) {
    SHARD_CROSS_QUERIES_TOTAL
        .with_label_values(&[operation])
        .inc();
}

/// Observe a query duration for a shard.
pub fn observe_query_duration(shard_id: u32, operation: &str, duration: Duration) {
    SHARD_QUERY_DURATION_SECONDS
        .with_label_values(&[&shard_id.to_string(), operation])
        .observe(duration.as_secs_f64());
}
