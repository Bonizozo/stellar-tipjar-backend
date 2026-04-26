use crate::errors::AppError;
use crate::service_mesh::circuit_breaker::CircuitBreaker;
use crate::service_mesh::discovery::ServiceRegistry;
use crate::service_mesh::load_balancer::{LoadBalancer, LoadBalancingStrategy};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Routes requests to backend service instances using service discovery,
/// load balancing, and per-service circuit breakers.
///
/// This is the core routing engine of the gateway.  It is used by the
/// `gateway_routing` middleware to resolve the upstream URL for a request.
pub struct ApiGateway {
    registry: Arc<ServiceRegistry>,
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<CircuitBreaker>>>>,
    load_balancer: Arc<LoadBalancer>,
}

impl ApiGateway {
    pub fn new(registry: Arc<ServiceRegistry>) -> Self {
        Self {
            registry,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            load_balancer: Arc::new(LoadBalancer::new(LoadBalancingStrategy::RoundRobin)),
        }
    }

    /// Resolve the upstream base URL for `service_name`.
    ///
    /// Returns an error when:
    /// - No instances are registered for the service.
    /// - The per-service circuit breaker is open.
    #[tracing::instrument(name = "gateway.route_request", skip(self), fields(service = %service_name))]
    pub async fn route_request(&self, service_name: &str) -> Result<String, AppError> {
        let instances = self.registry.discover_all(service_name).await;
        if instances.is_empty() {
            return Err(AppError::not_found(format!(
                "No instances registered for service '{}'",
                service_name
            )));
        }

        // Get or create a circuit breaker for this service.
        let breaker = {
            let mut breakers = self.circuit_breakers.write().await;
            breakers
                .entry(service_name.to_string())
                .or_insert_with(|| {
                    Arc::new(CircuitBreaker::new(
                        5,                          // failure threshold
                        2,                          // success threshold to close
                        Duration::from_secs(30),    // recovery window
                    ))
                })
                .clone()
        };

        breaker.check_half_open().await;

        if breaker.is_open().await {
            tracing::warn!(service = %service_name, "Circuit breaker open");
            return Err(AppError::service_unavailable(format!(
                "Service '{}' is temporarily unavailable (circuit breaker open)",
                service_name
            )));
        }

        let instance = self
            .load_balancer
            .select(&instances)
            .ok_or_else(|| AppError::service_unavailable("No healthy instances available"))?;

        breaker.record_success().await;

        let url = format!("http://{}:{}", instance.host, instance.port);
        tracing::debug!(service = %service_name, upstream = %url, "Routed request");
        Ok(url)
    }
}
