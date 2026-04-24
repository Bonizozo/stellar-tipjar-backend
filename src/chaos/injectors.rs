use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::ChaosError;

pub type Result<T> = std::result::Result<T, ChaosError>;

#[async_trait]
pub trait ChaosInjector: Send + Sync {
    async fn inject(&self) -> Result<()>;
    async fn recover(&self) -> Result<()>;
}

// ── Latency injector ──────────────────────────────────────────────────────────

pub struct LatencyInjector {
    pub target_service: String,
    pub latency: Duration,
    active: Arc<AtomicBool>,
}

impl LatencyInjector {
    pub fn new(target_service: impl Into<String>, latency: Duration) -> Self {
        Self {
            target_service: target_service.into(),
            latency,
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Applies the configured delay if injection is active.
    pub async fn maybe_delay(&self) {
        if self.is_active() {
            tokio::time::sleep(self.latency).await;
        }
    }
}

#[async_trait]
impl ChaosInjector for LatencyInjector {
    async fn inject(&self) -> Result<()> {
        self.active.store(true, Ordering::SeqCst);
        tracing::warn!(
            target = %self.target_service,
            latency_ms = self.latency.as_millis(),
            "Chaos: latency injection active"
        );
        Ok(())
    }

    async fn recover(&self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        tracing::info!(target = %self.target_service, "Chaos: latency injection removed");
        Ok(())
    }
}

// ── Database failure injector ─────────────────────────────────────────────────

pub struct DatabaseFailureInjector {
    pub failure_rate: f64,
    active: Arc<AtomicBool>,
}

impl DatabaseFailureInjector {
    pub fn new(failure_rate: f64) -> Self {
        Self {
            failure_rate: failure_rate.clamp(0.0, 1.0),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns `Err` with the configured probability when injection is active.
    pub fn maybe_fail(&self) -> Result<()> {
        if self.active.load(Ordering::SeqCst) && rand_f64() < self.failure_rate {
            return Err(ChaosError::InjectedFailure("database".into()));
        }
        Ok(())
    }
}

#[async_trait]
impl ChaosInjector for DatabaseFailureInjector {
    async fn inject(&self) -> Result<()> {
        self.active.store(true, Ordering::SeqCst);
        tracing::warn!(failure_rate = self.failure_rate, "Chaos: database failure injection active");
        Ok(())
    }

    async fn recover(&self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        tracing::info!("Chaos: database failure injection removed");
        Ok(())
    }
}

// ── Network partition injector ────────────────────────────────────────────────

pub struct NetworkPartitionInjector {
    pub target: String,
    active: Arc<AtomicBool>,
}

impl NetworkPartitionInjector {
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_partitioned(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ChaosInjector for NetworkPartitionInjector {
    async fn inject(&self) -> Result<()> {
        self.active.store(true, Ordering::SeqCst);
        tracing::warn!(target = %self.target, "Chaos: network partition active");
        Ok(())
    }

    async fn recover(&self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        tracing::info!(target = %self.target, "Chaos: network partition removed");
        Ok(())
    }
}

// ── Service crash injector ────────────────────────────────────────────────────

pub struct ServiceCrashInjector {
    pub service_name: String,
    crashed: Arc<AtomicBool>,
}

impl ServiceCrashInjector {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            crashed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_crashed(&self) -> bool {
        self.crashed.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ChaosInjector for ServiceCrashInjector {
    async fn inject(&self) -> Result<()> {
        self.crashed.store(true, Ordering::SeqCst);
        tracing::warn!(service = %self.service_name, "Chaos: service crash simulated");
        Ok(())
    }

    async fn recover(&self) -> Result<()> {
        self.crashed.store(false, Ordering::SeqCst);
        tracing::info!(service = %self.service_name, "Chaos: service restarted");
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Simple LCG-based pseudo-random f64 in [0, 1) without pulling in `rand`.
fn rand_f64() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(42);
    (seed as f64) / (u32::MAX as f64)
}
