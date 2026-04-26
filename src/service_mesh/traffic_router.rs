use crate::service_mesh::discovery::ServiceInstance;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A named traffic split between a stable and a canary pool.
pub struct TrafficRouter {
    /// Percentage of traffic (0–100) directed to the canary pool.
    canary_weight: Arc<AtomicUsize>,
    counter: Arc<AtomicUsize>,
}

impl TrafficRouter {
    pub fn new(canary_weight: usize) -> Self {
        Self {
            canary_weight: Arc::new(AtomicUsize::new(canary_weight.min(100))),
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Update the canary weight at runtime (0–100).
    pub fn set_canary_weight(&self, weight: usize) {
        self.canary_weight.store(weight.min(100), Ordering::SeqCst);
    }

    pub fn canary_weight(&self) -> usize {
        self.canary_weight.load(Ordering::SeqCst)
    }

    /// Route a request to either the canary or stable pool.
    /// Returns `None` if the chosen pool is empty.
    pub fn route<'a>(
        &self,
        stable: &'a [ServiceInstance],
        canary: &'a [ServiceInstance],
    ) -> Option<&'a ServiceInstance> {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        let weight = self.canary_weight.load(Ordering::SeqCst);

        let use_canary = !canary.is_empty() && (n % 100) < weight;
        if use_canary {
            canary.get(n % canary.len())
        } else {
            stable.get(n % stable.len().max(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_mesh::discovery::ServiceInstance;
    use uuid::Uuid;

    fn inst(name: &str) -> ServiceInstance {
        ServiceInstance {
            id: Uuid::new_v4(),
            name: name.into(),
            host: "127.0.0.1".into(),
            port: 8080,
            healthy: true,
        }
    }

    #[test]
    fn zero_canary_weight_always_routes_stable() {
        let router = TrafficRouter::new(0);
        let stable = vec![inst("stable")];
        let canary = vec![inst("canary")];
        for _ in 0..20 {
            let chosen = router.route(&stable, &canary).unwrap();
            assert_eq!(chosen.name, "stable");
        }
    }

    #[test]
    fn full_canary_weight_always_routes_canary() {
        let router = TrafficRouter::new(100);
        let stable = vec![inst("stable")];
        let canary = vec![inst("canary")];
        for _ in 0..20 {
            let chosen = router.route(&stable, &canary).unwrap();
            assert_eq!(chosen.name, "canary");
        }
    }

    #[test]
    fn set_canary_weight_updates_routing() {
        let router = TrafficRouter::new(0);
        let stable = vec![inst("stable")];
        let canary = vec![inst("canary")];
        router.set_canary_weight(100);
        let chosen = router.route(&stable, &canary).unwrap();
        assert_eq!(chosen.name, "canary");
    }
}
