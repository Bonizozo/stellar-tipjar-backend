use anyhow::Result;
use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HealthCheckError {
    #[error("Database connection failed: {0}")]
    DatabaseError(String),
    #[error("Redis connection failed: {0}")]
    RedisError(String),
    #[error("External service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("Health check timeout: {0}")]
    Timeout(String),
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl HealthStatus {
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    pub fn is_degraded(&self) -> bool {
        matches!(self, HealthStatus::Degraded)
    }

    pub fn is_unhealthy(&self) -> bool {
        matches!(self, HealthStatus::Unhealthy)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub service_name: String,
    pub status: HealthStatus,
    pub message: String,
    pub response_time_ms: u64,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

impl HealthCheckResult {
    pub fn healthy(service_name: String, response_time_ms: u64) -> Self {
        Self {
            service_name,
            status: HealthStatus::Healthy,
            message: "Service is operating normally".to_string(),
            response_time_ms,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn degraded(service_name: String, message: String, response_time_ms: u64) -> Self {
        Self {
            service_name,
            status: HealthStatus::Degraded,
            message,
            response_time_ms,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn unhealthy(service_name: String, message: String, response_time_ms: u64) -> Self {
        Self {
            service_name,
            status: HealthStatus::Unhealthy,
            message,
            response_time_ms,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub timeout_ms: u64,
    pub interval_seconds: u64,
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
    pub enabled_checks: Vec<String>,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            interval_seconds: 30,
            failure_threshold: 3,
            recovery_threshold: 2,
            enabled_checks: vec![
                "database".to_string(),
                "redis".to_string(),
                "stellar".to_string(),
                "disk_space".to_string(),
                "memory".to_string(),
            ],
        }
    }
}

pub trait HealthCheck: Send + Sync {
    async fn check(&self) -> Result<HealthCheckResult, HealthCheckError>;
    fn name(&self) -> &str;
}

pub struct DatabaseHealthCheck {
    pool: Arc<PgPool>,
    config: HealthCheckConfig,
}

impl DatabaseHealthCheck {
    pub fn new(pool: Arc<PgPool>, config: HealthCheckConfig) -> Self {
        Self { pool, config }
    }
}

impl HealthCheck for DatabaseHealthCheck {
    async fn check(&self) -> Result<HealthCheckResult, HealthCheckError> {
        let start = std::time::Instant::now();
        
        let result = tokio::time::timeout(
            Duration::from_millis(self.config.timeout_ms),
            async {
                sqlx::query("SELECT 1")
                    .fetch_one(self.pool.as_ref())
                    .await
            }
        ).await;

        let response_time = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(_)) => {
                let mut metadata = HashMap::new();
                if let Ok(stats) = self.pool.acquire().await {
                    metadata.insert("active_connections".to_string(), stats.stat().acquired().to_string());
                }

                Ok(HealthCheckResult::healthy("database".to_string(), response_time)
                    .with_metadata("connection_pool_size".to_string(), self.pool.size().to_string())
                    .with_metadata("idle_connections".to_string(), self.pool.num_idle().to_string()))
            }
            Ok(Err(e)) => Ok(HealthCheckResult::unhealthy(
                "database".to_string(),
                format!("Database query failed: {}", e),
                response_time,
            )),
            Err(_) => Ok(HealthCheckResult::unhealthy(
                "database".to_string(),
                "Database health check timed out".to_string(),
                response_time,
            )),
        }
    }

    fn name(&self) -> &str {
        "database"
    }
}

pub struct RedisHealthCheck {
    redis: Option<Arc<ConnectionManager>>,
    config: HealthCheckConfig,
}

impl RedisHealthCheck {
    pub fn new(redis: Option<Arc<ConnectionManager>>, config: HealthCheckConfig) -> Self {
        Self { redis, config }
    }
}

impl HealthCheck for RedisHealthCheck {
    async fn check(&self) -> Result<HealthCheckResult, HealthCheckError> {
        let start = std::time::Instant::now();

        if let Some(redis) = &self.redis {
            let result = tokio::time::timeout(
                Duration::from_millis(self.config.timeout_ms),
                async {
                    let mut conn = redis.clone();
                    redis::cmd("PING").query_async::<_, String>(&mut conn).await
                }
            ).await;

            let response_time = start.elapsed().as_millis() as u64;

            match result {
                Ok(Ok(response)) => {
                    if response == "PONG" {
                        Ok(HealthCheckResult::healthy("redis".to_string(), response_time))
                    } else {
                        Ok(HealthCheckResult::degraded(
                            "redis".to_string(),
                            format!("Unexpected Redis response: {}", response),
                            response_time,
                        ))
                    }
                }
                Ok(Err(e)) => Ok(HealthCheckResult::unhealthy(
                    "redis".to_string(),
                    format!("Redis ping failed: {}", e),
                    response_time,
                )),
                Err(_) => Ok(HealthCheckResult::unhealthy(
                    "redis".to_string(),
                    "Redis health check timed out".to_string(),
                    response_time,
                )),
            }
        } else {
            Ok(HealthCheckResult::degraded(
                "redis".to_string(),
                "Redis not configured".to_string(),
                start.elapsed().as_millis() as u64,
            ))
        }
    }

    fn name(&self) -> &str {
        "redis"
    }
}

pub struct StellarHealthCheck {
    rpc_url: String,
    config: HealthCheckConfig,
}

impl StellarHealthCheck {
    pub fn new(rpc_url: String, config: HealthCheckConfig) -> Self {
        Self { rpc_url, config }
    }
}

impl HealthCheck for StellarHealthCheck {
    async fn check(&self) -> Result<HealthCheckResult, HealthCheckError> {
        let start = std::time::Instant::now();

        let client = reqwest::Client::new();
        let url = format!("{}/health", self.rpc_url);

        let result = tokio::time::timeout(
            Duration::from_millis(self.config.timeout_ms),
            client.get(&url).send()
        ).await;

        let response_time = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(response)) => {
                if response.status().is_success() {
                    Ok(HealthCheckResult::healthy("stellar".to_string(), response_time))
                } else {
                    Ok(HealthCheckResult::degraded(
                        "stellar".to_string(),
                        format!("Stellar service returned status: {}", response.status()),
                        response_time,
                    ))
                }
            }
            Ok(Err(e)) => Ok(HealthCheckResult::unhealthy(
                "stellar".to_string(),
                format!("Stellar service request failed: {}", e),
                response_time,
            )),
            Err(_) => Ok(HealthCheckResult::unhealthy(
                "stellar".to_string(),
                "Stellar health check timed out".to_string(),
                response_time,
            )),
        }
    }

    fn name(&self) -> &str {
        "stellar"
    }
}

pub struct DiskSpaceHealthCheck {
    config: HealthCheckConfig,
    warning_threshold_bytes: u64,
    critical_threshold_bytes: u64,
}

impl DiskSpaceHealthCheck {
    pub fn new(config: HealthCheckConfig, warning_threshold_bytes: u64, critical_threshold_bytes: u64) -> Self {
        Self {
            config,
            warning_threshold_bytes,
            critical_threshold_bytes,
        }
    }
}

impl HealthCheck for DiskSpaceHealthCheck {
    async fn check(&self) -> Result<HealthCheckResult, HealthCheckError> {
        let start = std::time::Instant::now();

        match self.get_disk_space().await {
            Ok((total_bytes, free_bytes)) => {
                let used_percentage = ((total_bytes - free_bytes) as f64 / total_bytes as f64) * 100.0;
                let response_time = start.elapsed().as_millis() as u64;

                let mut metadata = HashMap::new();
                metadata.insert("total_bytes".to_string(), total_bytes.to_string());
                metadata.insert("free_bytes".to_string(), free_bytes.to_string());
                metadata.insert("used_percentage".to_string(), format!("{:.2}", used_percentage));

                if free_bytes < self.critical_threshold_bytes {
                    Ok(HealthCheckResult::unhealthy(
                        "disk_space".to_string(),
                        format!("Critical disk space: {:.2}% used", used_percentage),
                        response_time,
                    ))
                } else if free_bytes < self.warning_threshold_bytes {
                    Ok(HealthCheckResult::degraded(
                        "disk_space".to_string(),
                        format!("Low disk space: {:.2}% used", used_percentage),
                        response_time,
                    ))
                } else {
                    Ok(HealthCheckResult::healthy("disk_space".to_string(), response_time))
                }
            }
            Err(e) => Ok(HealthCheckResult::unhealthy(
                "disk_space".to_string(),
                format!("Failed to check disk space: {}", e),
                start.elapsed().as_millis() as u64,
            )),
        }
    }

    fn name(&self) -> &str {
        "disk_space"
    }
}

impl DiskSpaceHealthCheck {
    async fn get_disk_space(&self) -> Result<(u64, u64), HealthCheckError> {
        // This is a simplified implementation
        // In a real implementation, you would use sysinfo or similar crate
        use std::fs;
        
        let current_dir = std::env::current_dir()
            .map_err(|e| HealthCheckError::ConfigurationError(e.to_string()))?;
        
        let metadata = fs::metadata(&current_dir)
            .map_err(|e| HealthCheckError::ConfigurationError(e.to_string()))?;

        // For demonstration, return dummy values
        // In production, you'd use a proper disk space checking library
        let total_bytes = 100_000_000_000u64; // 100GB
        let free_bytes = 50_000_000_000u64;  // 50GB

        Ok((total_bytes, free_bytes))
    }
}

pub struct MemoryHealthCheck {
    config: HealthCheckConfig,
    warning_threshold_percentage: f64,
    critical_threshold_percentage: f64,
}

impl MemoryHealthCheck {
    pub fn new(config: HealthCheckConfig, warning_threshold_percentage: f64, critical_threshold_percentage: f64) -> Self {
        Self {
            config,
            warning_threshold_percentage,
            critical_threshold_percentage,
        }
    }
}

impl HealthCheck for MemoryHealthCheck {
    async fn check(&self) -> Result<HealthCheckResult, HealthCheckError> {
        let start = std::time::Instant::now();

        match self.get_memory_usage().await {
            Ok((total_bytes, used_bytes)) => {
                let used_percentage = (used_bytes as f64 / total_bytes as f64) * 100.0;
                let response_time = start.elapsed().as_millis() as u64;

                let mut metadata = HashMap::new();
                metadata.insert("total_bytes".to_string(), total_bytes.to_string());
                metadata.insert("used_bytes".to_string(), used_bytes.to_string());
                metadata.insert("used_percentage".to_string(), format!("{:.2}", used_percentage));

                if used_percentage > self.critical_threshold_percentage {
                    Ok(HealthCheckResult::unhealthy(
                        "memory".to_string(),
                        format!("Critical memory usage: {:.2}%", used_percentage),
                        response_time,
                    ))
                } else if used_percentage > self.warning_threshold_percentage {
                    Ok(HealthCheckResult::degraded(
                        "memory".to_string(),
                        format!("High memory usage: {:.2}%", used_percentage),
                        response_time,
                    ))
                } else {
                    Ok(HealthCheckResult::healthy("memory".to_string(), response_time))
                }
            }
            Err(e) => Ok(HealthCheckResult::unhealthy(
                "memory".to_string(),
                format!("Failed to check memory usage: {}", e),
                start.elapsed().as_millis() as u64,
            )),
        }
    }

    fn name(&self) -> &str {
        "memory"
    }
}

impl MemoryHealthCheck {
    async fn get_memory_usage(&self) -> Result<(u64, u64), HealthCheckError> {
        // This is a simplified implementation
        // In a real implementation, you would use sysinfo or similar crate
        
        // For demonstration, return dummy values
        // In production, you'd use a proper memory checking library
        let total_bytes = 8_000_000_000u64; // 8GB
        let used_bytes = 4_000_000_000u64;   // 4GB

        Ok((total_bytes, used_bytes))
    }
}

pub struct HealthCheckRegistry {
    checks: HashMap<String, Box<dyn HealthCheck>>,
    config: HealthCheckConfig,
}

impl HealthCheckRegistry {
    pub fn new(config: HealthCheckConfig) -> Self {
        Self {
            checks: HashMap::new(),
            config,
        }
    }

    pub fn register_check(&mut self, check: Box<dyn HealthCheck>) {
        let name = check.name().to_string();
        self.checks.insert(name, check);
    }

    pub async fn run_all_checks(&self) -> Vec<HealthCheckResult> {
        let mut results = Vec::new();

        for (name, check) in &self.checks {
            if self.config.enabled_checks.contains(name) {
                match check.check().await {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        results.push(HealthCheckResult::unhealthy(
                            name.clone(),
                            format!("Health check failed: {}", e),
                            0,
                        ));
                    }
                }
            }
        }

        results
    }

    pub async fn run_check(&self, name: &str) -> Option<HealthCheckResult> {
        if let Some(check) = self.checks.get(name) {
            match check.check().await {
                Ok(result) => Some(result),
                Err(e) => Some(HealthCheckResult::unhealthy(
                    name.to_string(),
                    format!("Health check failed: {}", e),
                    0,
                )),
            }
        } else {
            None
        }
    }

    pub fn get_check_names(&self) -> Vec<String> {
        self.checks.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Degraded.is_healthy());
        assert!(!HealthStatus::Unhealthy.is_healthy());
        assert!(HealthStatus::Degraded.is_degraded());
        assert!(!HealthStatus::Healthy.is_degraded());
    }

    #[test]
    fn test_health_check_result_creation() {
        let result = HealthCheckResult::healthy("test".to_string(), 100);
        assert_eq!(result.service_name, "test");
        assert_eq!(result.response_time_ms, 100);
        assert!(result.status.is_healthy());

        let result = HealthCheckResult::degraded("test".to_string(), "warning".to_string(), 200);
        assert_eq!(result.service_name, "test");
        assert_eq!(result.message, "warning");
        assert!(result.status.is_degraded());

        let result = HealthCheckResult::unhealthy("test".to_string(), "error".to_string(), 300);
        assert_eq!(result.service_name, "test");
        assert_eq!(result.message, "error");
        assert!(result.status.is_unhealthy());
    }

    #[test]
    fn test_health_check_registry() {
        let config = HealthCheckConfig::default();
        let mut registry = HealthCheckRegistry::new(config);
        
        assert_eq!(registry.get_check_names().len(), 0);
        
        // Note: Can't easily test without mock implementations
        // In real tests, you'd use mock health checks
    }
}
