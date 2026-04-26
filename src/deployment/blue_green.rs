use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Blue,
    Green,
}

impl Slot {
    pub fn other(self) -> Self {
        match self {
            Slot::Blue => Slot::Green,
            Slot::Green => Slot::Blue,
        }
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slot::Blue => write!(f, "blue"),
            Slot::Green => write!(f, "green"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlotConfig {
    pub host: String,
    pub port: u16,
}

/// Manages a blue-green deployment: tracks which slot is live, supports
/// atomic traffic switching, rollback, and smoke-test gating.
pub struct BlueGreenManager {
    live: Arc<RwLock<Slot>>,
    blue: SlotConfig,
    green: SlotConfig,
}

impl BlueGreenManager {
    pub fn new(blue: SlotConfig, green: SlotConfig, initial: Slot) -> Self {
        Self {
            live: Arc::new(RwLock::new(initial)),
            blue,
            green,
        }
    }

    pub async fn live_slot(&self) -> Slot {
        *self.live.read().await
    }

    pub async fn live_config(&self) -> SlotConfig {
        match self.live_slot().await {
            Slot::Blue => self.blue.clone(),
            Slot::Green => self.green.clone(),
        }
    }

    pub async fn standby_config(&self) -> SlotConfig {
        match self.live_slot().await {
            Slot::Blue => self.green.clone(),
            Slot::Green => self.blue.clone(),
        }
    }

    /// Run `smoke_test` against the standby slot. If it passes, switch traffic.
    /// Returns the new live slot on success, or an error string on failure.
    pub async fn deploy<F, Fut>(&self, smoke_test: F) -> Result<Slot, String>
    where
        F: FnOnce(SlotConfig) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let standby_cfg = self.standby_config().await;
        let standby_slot = self.live_slot().await.other();

        tracing::info!(slot = %standby_slot, "Running smoke test before traffic switch");
        if !smoke_test(standby_cfg).await {
            return Err(format!(
                "Smoke test failed for slot {standby_slot}; deployment aborted"
            ));
        }

        self.switch().await;
        tracing::info!(slot = %standby_slot, "Traffic switched to new slot");
        Ok(standby_slot)
    }

    /// Immediately roll back to the previous (standby) slot without a smoke test.
    pub async fn rollback(&self) -> Slot {
        self.switch().await;
        let slot = self.live_slot().await;
        tracing::warn!(slot = %slot, "Rolled back to previous slot");
        slot
    }

    /// Health check: returns `true` when the live slot responds within `timeout`.
    pub async fn health_check(&self, timeout: Duration) -> bool {
        let cfg = self.live_config().await;
        let url = format!("http://{}:{}/health", cfg.host, cfg.port);
        match tokio::time::timeout(timeout, reqwest::get(&url)).await {
            Ok(Ok(resp)) => resp.status().is_success(),
            _ => false,
        }
    }

    async fn switch(&self) {
        let mut live = self.live.write().await;
        *live = live.other();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> BlueGreenManager {
        BlueGreenManager::new(
            SlotConfig { host: "blue.local".into(), port: 8080 },
            SlotConfig { host: "green.local".into(), port: 8081 },
            Slot::Blue,
        )
    }

    #[tokio::test]
    async fn initial_slot_is_blue() {
        assert_eq!(manager().live_slot().await, Slot::Blue);
    }

    #[tokio::test]
    async fn deploy_switches_slot_when_smoke_test_passes() {
        let mgr = manager();
        let new_slot = mgr.deploy(|_cfg| async { true }).await.unwrap();
        assert_eq!(new_slot, Slot::Green);
        assert_eq!(mgr.live_slot().await, Slot::Green);
    }

    #[tokio::test]
    async fn deploy_aborts_when_smoke_test_fails() {
        let mgr = manager();
        let result = mgr.deploy(|_cfg| async { false }).await;
        assert!(result.is_err());
        assert_eq!(mgr.live_slot().await, Slot::Blue, "Slot must not change on failure");
    }

    #[tokio::test]
    async fn rollback_reverts_to_previous_slot() {
        let mgr = manager();
        mgr.deploy(|_cfg| async { true }).await.unwrap(); // now Green
        let rolled_back = mgr.rollback().await;
        assert_eq!(rolled_back, Slot::Blue);
        assert_eq!(mgr.live_slot().await, Slot::Blue);
    }

    #[test]
    fn slot_other_is_symmetric() {
        assert_eq!(Slot::Blue.other(), Slot::Green);
        assert_eq!(Slot::Green.other(), Slot::Blue);
    }
}
