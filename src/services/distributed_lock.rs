//! Distributed locking via Redis SET NX PX (Redlock-style single-node).
//!
//! Each lock is stored as:
//!   key  = `lock:<resource>`
//!   value = `<owner_token>` (random UUID, proves ownership)
//!   TTL  = configurable (default 30 s)
//!
//! Deadlock detection: a background task scans for lock keys whose TTL has
//! dropped to ≤ 0 (i.e. Redis already expired them) and emits a warning.
//! Because Redis auto-expires keys, true deadlocks cannot occur — the
//! detection here tracks *stale* lock metadata kept in a monitoring hash.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use redis::AsyncCommands;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{info, warn};
use uuid::Uuid;

use crate::cache::redis_client::ConnectionManager;

const KEY_PREFIX: &str = "lock:";
const MONITOR_HASH: &str = "lock:_monitor";

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Lock is held by another owner.
    AlreadyHeld,
    /// Redis is unavailable.
    Unavailable,
    /// Caller does not own the lock (wrong token).
    NotOwner,
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyHeld => write!(f, "lock already held"),
            Self::Unavailable => write!(f, "redis unavailable"),
            Self::NotOwner => write!(f, "caller does not own the lock"),
        }
    }
}

/// A held lock token. Drop it (or call `release`) to free the lock.
#[derive(Debug, Clone)]
pub struct LockGuard {
    pub resource: String,
    pub token: String,
    pub ttl_ms: u64,
}

/// Per-lock monitoring metadata stored in the in-process registry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LockInfo {
    pub resource: String,
    pub token: String,
    pub acquired_at: chrono::DateTime<chrono::Utc>,
    pub ttl_ms: u64,
}

/// Monitoring snapshot.
#[derive(Debug, serde::Serialize)]
pub struct LockStats {
    pub active_locks: Vec<LockInfo>,
    pub total_acquired: u64,
    pub total_released: u64,
    pub total_expired: u64,
}

// ── Service ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct DistributedLockService {
    redis: ConnectionManager,
    /// In-process registry for monitoring (resource → LockInfo).
    registry: Arc<RwLock<HashMap<String, LockInfo>>>,
    acquired: Arc<std::sync::atomic::AtomicU64>,
    released: Arc<std::sync::atomic::AtomicU64>,
    expired: Arc<std::sync::atomic::AtomicU64>,
}

impl DistributedLockService {
    pub fn new(redis: ConnectionManager) -> Self {
        Self {
            redis,
            registry: Arc::new(RwLock::new(HashMap::new())),
            acquired: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            released: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            expired: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Attempt to acquire `resource` for `ttl_ms` milliseconds.
    /// Returns `LockGuard` on success, `LockError::AlreadyHeld` if taken.
    pub async fn acquire(&self, resource: &str, ttl_ms: u64) -> Result<LockGuard, LockError> {
        let key = format!("{}{}", KEY_PREFIX, resource);
        let token = Uuid::new_v4().to_string();

        let result: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(ttl_ms)
            .query_async(&mut self.redis.clone())
            .await
            .map_err(|_| LockError::Unavailable)?;

        if result.as_deref() != Some("OK") {
            return Err(LockError::AlreadyHeld);
        }

        let guard = LockGuard {
            resource: resource.to_string(),
            token: token.clone(),
            ttl_ms,
        };

        // Register for monitoring.
        self.registry.write().await.insert(
            resource.to_string(),
            LockInfo {
                resource: resource.to_string(),
                token,
                acquired_at: chrono::Utc::now(),
                ttl_ms,
            },
        );
        self.acquired.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(guard)
    }

    /// Release a lock. Only succeeds if the caller still owns it (token match).
    /// Uses a Lua script for atomic check-and-delete.
    pub async fn release(&self, guard: &LockGuard) -> Result<(), LockError> {
        let key = format!("{}{}", KEY_PREFIX, guard.resource);

        // Atomic: only delete if value matches our token.
        let script = r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                return redis.call("DEL", KEYS[1])
            else
                return 0
            end
        "#;

        let deleted: i64 = redis::Script::new(script)
            .key(&key)
            .arg(&guard.token)
            .invoke_async(&mut self.redis.clone())
            .await
            .map_err(|_| LockError::Unavailable)?;

        if deleted == 0 {
            return Err(LockError::NotOwner);
        }

        self.registry.write().await.remove(&guard.resource);
        self.released.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Renew the TTL of a held lock. Only succeeds if the caller still owns it.
    pub async fn renew(&self, guard: &LockGuard, new_ttl_ms: u64) -> Result<(), LockError> {
        let key = format!("{}{}", KEY_PREFIX, guard.resource);

        let script = r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                return redis.call("PEXPIRE", KEYS[1], ARGV[2])
            else
                return 0
            end
        "#;

        let ok: i64 = redis::Script::new(script)
            .key(&key)
            .arg(&guard.token)
            .arg(new_ttl_ms)
            .invoke_async(&mut self.redis.clone())
            .await
            .map_err(|_| LockError::Unavailable)?;

        if ok == 0 {
            return Err(LockError::NotOwner);
        }

        // Update registry TTL.
        if let Some(info) = self.registry.write().await.get_mut(&guard.resource) {
            info.ttl_ms = new_ttl_ms;
        }

        Ok(())
    }

    /// Check whether a resource is currently locked.
    pub async fn is_locked(&self, resource: &str) -> bool {
        let key = format!("{}{}", KEY_PREFIX, resource);
        let val: Option<String> = self.redis.clone().get(&key).await.unwrap_or(None);
        val.is_some()
    }

    /// Monitoring snapshot.
    pub async fn stats(&self) -> LockStats {
        let active_locks: Vec<LockInfo> = self.registry.read().await.values().cloned().collect();
        LockStats {
            active_locks,
            total_acquired: self.acquired.load(std::sync::atomic::Ordering::Relaxed),
            total_released: self.released.load(std::sync::atomic::Ordering::Relaxed),
            total_expired: self.expired.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Background task: scan the in-process registry for locks whose Redis key
    /// has already expired (deadlock / abandoned lock detection).
    async fn detect_expired(&self) {
        let keys: Vec<String> = self.registry.read().await.keys().cloned().collect();
        for resource in keys {
            if !self.is_locked(&resource).await {
                // Redis key gone — lock expired without explicit release.
                if self.registry.write().await.remove(&resource).is_some() {
                    self.expired.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    warn!(resource = %resource, "Distributed lock expired without release (possible deadlock)");
                }
            }
        }
    }
}

// ── Background monitor ────────────────────────────────────────────────────────

/// Spawn a background task that periodically detects expired/abandoned locks.
pub fn spawn_monitor(svc: Arc<DistributedLockService>) {
    let poll_secs: u64 = std::env::var("LOCK_MONITOR_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    tokio::spawn(async move {
        info!(poll_secs, "Distributed lock monitor started");
        let mut ticker = interval(Duration::from_secs(poll_secs));
        loop {
            ticker.tick().await;
            svc.detect_expired().await;
        }
    });
}
