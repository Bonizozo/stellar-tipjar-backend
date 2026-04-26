use crate::service_mesh::discovery::ServiceRegistry;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ServiceHealth {
    pub name: String,
    pub total: usize,
    pub healthy: usize,
}

/// Snapshot the health of every registered service.
pub async fn mesh_health(registry: &ServiceRegistry) -> Vec<ServiceHealth> {
    // We iterate over known service names by discovering all instances.
    // In practice the registry would expose an `all_names()` helper; here we
    // use the public `discover_all` API with a fixed set of well-known names.
    let known = ["stellar-tipjar-backend", "stellar-horizon", "database", "redis"];
    let mut out = Vec::new();
    for name in known {
        let instances = registry.discover_all(name).await;
        if instances.is_empty() {
            continue;
        }
        let healthy = instances.iter().filter(|i| i.healthy).count();
        out.push(ServiceHealth {
            name: name.to_string(),
            total: instances.len(),
            healthy,
        });
    }
    out
}
