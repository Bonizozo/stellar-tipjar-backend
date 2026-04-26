use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use super::injectors::{ChaosInjector, Result};
use super::ChaosError;

/// Simulates resource exhaustion by tracking an artificial "connection pool" counter.
/// When active, every call to `try_acquire` that would exceed `max_connections`
/// returns an error, mimicking pool exhaustion.
pub struct ResourceExhaustionInjector {
    pub max_connections: usize,
    active: Arc<AtomicBool>,
    in_use: Arc<AtomicUsize>,
}

impl ResourceExhaustionInjector {
    pub fn new(max_connections: usize) -> Self {
        Self {
            max_connections,
            active: Arc::new(AtomicBool::new(false)),
            in_use: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Attempt to acquire a simulated connection.
    /// Returns `Err` when the pool is exhausted and injection is active.
    pub fn try_acquire(&self) -> Result<ResourceGuard> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(ResourceGuard { counter: Arc::clone(&self.in_use) });
        }
        let current = self.in_use.fetch_add(1, Ordering::SeqCst);
        if current >= self.max_connections {
            self.in_use.fetch_sub(1, Ordering::SeqCst);
            return Err(ChaosError::InjectedFailure("resource pool exhausted".into()));
        }
        Ok(ResourceGuard { counter: Arc::clone(&self.in_use) })
    }
}

/// RAII guard that releases the simulated connection on drop.
pub struct ResourceGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl ChaosInjector for ResourceExhaustionInjector {
    async fn inject(&self) -> Result<()> {
        self.active.store(true, Ordering::SeqCst);
        tracing::warn!(
            max_connections = self.max_connections,
            "Chaos: resource exhaustion injection active"
        );
        Ok(())
    }

    async fn recover(&self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        self.in_use.store(0, Ordering::SeqCst);
        tracing::info!("Chaos: resource exhaustion injection removed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chaos::injectors::ChaosInjector;

    #[tokio::test]
    async fn exhaustion_blocks_when_active_and_full() {
        let inj = ResourceExhaustionInjector::new(2);
        inj.inject().await.unwrap();

        let _g1 = inj.try_acquire().unwrap();
        let _g2 = inj.try_acquire().unwrap();
        assert!(inj.try_acquire().is_err());
    }

    #[tokio::test]
    async fn exhaustion_allows_after_guard_dropped() {
        let inj = ResourceExhaustionInjector::new(1);
        inj.inject().await.unwrap();

        {
            let _g = inj.try_acquire().unwrap();
            assert!(inj.try_acquire().is_err());
        } // guard dropped here

        assert!(inj.try_acquire().is_ok());
    }

    #[tokio::test]
    async fn no_exhaustion_when_inactive() {
        let inj = ResourceExhaustionInjector::new(0); // pool size 0
        // injection not active → always succeeds
        assert!(inj.try_acquire().is_ok());
    }
}
