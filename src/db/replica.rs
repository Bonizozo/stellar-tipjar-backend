use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Tracks health and lag for a single replica.
#[derive(Debug)]
struct ReplicaState {
    pool: PgPool,
    url: String,
    healthy: bool,
    lag_bytes: i64,
}

/// Round-robin read replica pool with lag monitoring and failure handling.
#[derive(Debug, Clone)]
pub struct ReplicaManager {
    replicas: Arc<RwLock<Vec<ReplicaState>>>,
    counter: Arc<AtomicUsize>,
    /// Maximum acceptable replication lag in bytes before a replica is excluded.
    max_lag_bytes: i64,
}

impl ReplicaManager {
    pub fn new(max_lag_bytes: i64) -> Self {
        Self {
            replicas: Arc::new(RwLock::new(Vec::new())),
            counter: Arc::new(AtomicUsize::new(0)),
            max_lag_bytes,
        }
    }

    /// Add a replica pool (called during startup for each DATABASE_REPLICA_URL_n).
    pub async fn add_replica(&self, url: String, pool: PgPool) {
        let mut replicas = self.replicas.write().await;
        replicas.push(ReplicaState {
            pool,
            url,
            healthy: true,
            lag_bytes: 0,
        });
    }

    /// Returns a healthy replica pool using round-robin, or `None` if none available.
    pub async fn get_replica(&self) -> Option<PgPool> {
        let replicas = self.replicas.read().await;
        let healthy: Vec<&ReplicaState> = replicas
            .iter()
            .filter(|r| r.healthy && r.lag_bytes <= self.max_lag_bytes)
            .collect();

        if healthy.is_empty() {
            return None;
        }

        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % healthy.len();
        Some(healthy[idx].pool.clone())
    }

    /// Returns replica count (total / healthy).
    pub async fn stats(&self) -> ReplicaStats {
        let replicas = self.replicas.read().await;
        let total = replicas.len();
        let healthy = replicas
            .iter()
            .filter(|r| r.healthy && r.lag_bytes <= self.max_lag_bytes)
            .count();
        let lags: Vec<ReplicaLag> = replicas
            .iter()
            .map(|r| ReplicaLag {
                url: r.url.clone(),
                healthy: r.healthy,
                lag_bytes: r.lag_bytes,
            })
            .collect();
        ReplicaStats {
            total,
            healthy,
            replicas: lags,
        }
    }

    /// Background task: poll each replica for replication lag and health.
    pub async fn monitor_loop(self, primary: PgPool, interval: Duration) {
        loop {
            tokio::time::sleep(interval).await;
            self.refresh_lag(&primary).await;
        }
    }

    async fn refresh_lag(&self, primary: &PgPool) {
        // Query primary for lag per replica using pg_stat_replication.
        let rows = sqlx::query!(
            r#"SELECT client_addr::text as "addr?", write_lag_bytes as "lag?"
               FROM pg_stat_replication"#
        )
        .fetch_all(primary)
        .await;

        let mut replicas = self.replicas.write().await;

        // Mark all unhealthy first, then restore based on connectivity check.
        for replica in replicas.iter_mut() {
            // Ping the replica directly.
            match sqlx::query("SELECT 1").execute(&replica.pool).await {
                Ok(_) => {
                    replica.healthy = true;
                }
                Err(e) => {
                    tracing::warn!(url = %replica.url, error = %e, "Replica health check failed");
                    replica.healthy = false;
                }
            }

            // Update lag from primary's pg_stat_replication if available.
            if let Ok(ref rows) = rows {
                for row in rows {
                    if let (Some(addr), Some(lag)) = (&row.addr, row.lag) {
                        if replica.url.contains(addr.as_str()) {
                            replica.lag_bytes = lag;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ReplicaStats {
    pub total: usize,
    pub healthy: usize,
    pub replicas: Vec<ReplicaLag>,
}

#[derive(Debug, serde::Serialize)]
pub struct ReplicaLag {
    pub url: String,
    pub healthy: bool,
    pub lag_bytes: i64,
}
