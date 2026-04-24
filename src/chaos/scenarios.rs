use std::time::Duration;

use super::experiments::ChaosExperiment;
use super::injectors::{
    DatabaseFailureInjector, LatencyInjector, NetworkPartitionInjector, ServiceCrashInjector,
};

/// Pre-built scenarios that cover the most common failure modes.
pub struct ChaosScenarios;

impl ChaosScenarios {
    /// Simulates a complete database outage (100 % failure rate).
    pub fn database_outage() -> ChaosExperiment {
        ChaosExperiment::new("Database Outage", Duration::from_secs(30))
            .with_injector(Box::new(DatabaseFailureInjector::new(1.0)))
    }

    /// Simulates intermittent database failures (50 % failure rate).
    pub fn database_flakiness() -> ChaosExperiment {
        ChaosExperiment::new("Database Flakiness", Duration::from_secs(30))
            .with_injector(Box::new(DatabaseFailureInjector::new(0.5)))
    }

    /// Simulates a network partition to the Stellar Horizon API.
    pub fn stellar_network_partition() -> ChaosExperiment {
        ChaosExperiment::new("Stellar Network Partition", Duration::from_secs(30))
            .with_injector(Box::new(NetworkPartitionInjector::new("stellar-horizon")))
    }

    /// Simulates high latency on the database connection (500 ms).
    pub fn high_database_latency() -> ChaosExperiment {
        ChaosExperiment::new("High Database Latency", Duration::from_secs(30))
            .with_injector(Box::new(LatencyInjector::new(
                "database",
                Duration::from_millis(500),
            )))
    }

    /// Simulates a crash of the tip-processing service.
    pub fn tip_service_crash() -> ChaosExperiment {
        ChaosExperiment::new("Tip Service Crash", Duration::from_secs(15))
            .with_injector(Box::new(ServiceCrashInjector::new("tip-service")))
    }

    /// Combined scenario: high latency + partial database failures.
    pub fn degraded_mode() -> ChaosExperiment {
        ChaosExperiment::new("Degraded Mode", Duration::from_secs(30))
            .with_injector(Box::new(LatencyInjector::new(
                "database",
                Duration::from_millis(200),
            )))
            .with_injector(Box::new(DatabaseFailureInjector::new(0.2)))
    }

    /// Returns all standard scenarios for a full resilience suite.
    pub fn all() -> Vec<ChaosExperiment> {
        vec![
            Self::database_outage(),
            Self::database_flakiness(),
            Self::stellar_network_partition(),
            Self::high_database_latency(),
            Self::tip_service_crash(),
            Self::degraded_mode(),
        ]
    }
}
