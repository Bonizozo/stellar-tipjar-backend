use crate::service_mesh::discovery::ServiceInstance;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

pub enum LoadBalancingStrategy {
    RoundRobin,
    LeastConnections,
    Random,
}

pub struct LoadBalancer {
    strategy: LoadBalancingStrategy,
    counter: Arc<AtomicUsize>,
    /// active_connections[instance_id] = count (for LeastConnections)
    active_connections: Arc<RwLock<HashMap<String, usize>>>,
    /// sticky_sessions[session_key] = instance_id
    sticky_sessions: Arc<RwLock<HashMap<String, String>>>,
}

impl LoadBalancer {
    pub fn new(strategy: LoadBalancingStrategy) -> Self {
        Self {
            strategy,
            counter: Arc::new(AtomicUsize::new(0)),
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            sticky_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Select a healthy instance, respecting sticky session if provided.
    pub async fn select(
        &self,
        instances: &[ServiceInstance],
        session_key: Option<&str>,
    ) -> Option<ServiceInstance> {
        let healthy: Vec<&ServiceInstance> = instances.iter().filter(|i| i.healthy).collect();
        if healthy.is_empty() {
            return None;
        }

        // Sticky session: return the pinned instance if it's still healthy.
        if let Some(key) = session_key {
            let sessions = self.sticky_sessions.read().await;
            if let Some(pinned_id) = sessions.get(key) {
                if let Some(inst) = healthy.iter().find(|i| i.id.to_string() == *pinned_id) {
                    return Some((*inst).clone());
                }
            }
        }

        let chosen = match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                let idx = self.counter.fetch_add(1, Ordering::SeqCst) % healthy.len();
                (*healthy[idx]).clone()
            }
            LoadBalancingStrategy::LeastConnections => {
                let conns = self.active_connections.read().await;
                healthy
                    .iter()
                    .min_by_key(|i| conns.get(&i.id.to_string()).copied().unwrap_or(0))
                    .map(|i| (*i).clone())?
            }
            LoadBalancingStrategy::Random => {
                let idx = (self.counter.fetch_add(1, Ordering::SeqCst) * 7919) % healthy.len();
                (*healthy[idx]).clone()
            }
        };

        // Pin to session if a key was provided.
        if let Some(key) = session_key {
            self.sticky_sessions
                .write()
                .await
                .insert(key.to_string(), chosen.id.to_string());
        }

        Some(chosen)
    }

    /// Record that a request to `instance_id` started.
    pub async fn on_request_start(&self, instance_id: &str) {
        *self
            .active_connections
            .write()
            .await
            .entry(instance_id.to_string())
            .or_insert(0) += 1;
    }

    /// Record that a request to `instance_id` finished.
    pub async fn on_request_end(&self, instance_id: &str) {
        let mut conns = self.active_connections.write().await;
        let count = conns.entry(instance_id.to_string()).or_insert(0);
        *count = count.saturating_sub(1);
    }

    /// Remove sticky session binding for a key (e.g. on logout).
    pub async fn clear_session(&self, session_key: &str) {
        self.sticky_sessions.write().await.remove(session_key);
    }
}

/// Performs a lightweight HTTP health check against an instance.
pub async fn check_instance_health(instance: &ServiceInstance) -> bool {
    let url = format!("http://{}:{}/health", instance.host, instance.port);
    match reqwest::get(&url).await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Refreshes the `healthy` flag on every instance in the slice.
pub async fn refresh_health(instances: &mut Vec<ServiceInstance>) {
    for instance in instances.iter_mut() {
        instance.healthy = check_instance_health(instance).await;
        if !instance.healthy {
            tracing::warn!(
                id = %instance.id,
                host = %instance.host,
                port = instance.port,
                "Instance marked unhealthy"
            );
        }
    }
}
