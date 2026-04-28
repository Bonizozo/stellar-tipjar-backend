use anyhow::Result;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use super::checks::{HealthCheckRegistry, HealthCheckResult, HealthStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealth {
    pub overall_status: HealthStatus,
    pub checks: Vec<HealthCheckResult>,
    pub timestamp: DateTime<Utc>,
    pub uptime_percentage: f64,
    pub last_check_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthHistory {
    pub service_name: String,
    pub status_history: VecDeque<(DateTime<Utc>, HealthStatus)>,
    pub max_history_size: usize,
}

impl HealthHistory {
    pub fn new(service_name: String, max_history_size: usize) -> Self {
        Self {
            service_name,
            status_history: VecDeque::with_capacity(max_history_size),
            max_history_size,
        }
    }

    pub fn add_result(&mut self, timestamp: DateTime<Utc>, status: HealthStatus) {
        self.status_history.push_back((timestamp, status));
        
        if self.status_history.len() > self.max_history_size {
            self.status_history.pop_front();
        }
    }

    pub fn calculate_uptime(&self, period_hours: i64) -> f64 {
        if self.status_history.is_empty() {
            return 0.0;
        }

        let cutoff = Utc::now() - Duration::hours(period_hours);
        let recent_checks: Vec<_> = self.status_history
            .iter()
            .filter(|(timestamp, _)| *timestamp >= cutoff)
            .collect();

        if recent_checks.is_empty() {
            return 0.0;
        }

        let healthy_count = recent_checks
            .iter()
            .filter(|(_, status)| status.is_healthy())
            .count();

        (healthy_count as f64 / recent_checks.len() as f64) * 100.0
    }

    pub fn get_current_status(&self) -> HealthStatus {
        self.status_history
            .back()
            .map(|(_, status)| status.clone())
            .unwrap_or(HealthStatus::Unknown)
    }

    pub fn get_status_trend(&self) -> String {
        if self.status_history.len() < 2 {
            return "stable".to_string();
        }

        let recent: Vec<_> = self.status_history
            .iter()
            .rev()
            .take(5)
            .collect();

        let healthy_count = recent
            .iter()
            .filter(|(_, status)| status.is_healthy())
            .count();

        if healthy_count == recent.len() {
            "improving".to_string()
        } else if healthy_count == 0 {
            "degrading".to_string()
        } else if healthy_count >= recent.len() / 2 {
            "stable".to_string()
        } else {
            "fluctuating".to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthAlert {
    pub id: String,
    pub service_name: String,
    pub alert_type: AlertType,
    pub message: String,
    pub severity: AlertSeverity,
    pub timestamp: DateTime<Utc>,
    pub resolved: bool,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    ServiceDown,
    ServiceDegraded,
    ServiceRecovered,
    HighResponseTime,
    ThresholdExceeded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl HealthAlert {
    pub fn new(
        service_name: String,
        alert_type: AlertType,
        message: String,
        severity: AlertSeverity,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            service_name,
            alert_type,
            message,
            severity,
            timestamp: Utc::now(),
            resolved: false,
            resolved_at: None,
        }
    }

    pub fn resolve(&mut self) {
        self.resolved = true;
        self.resolved_at = Some(Utc::now());
    }
}

pub struct HealthMonitor {
    registry: Arc<HealthCheckRegistry>,
    health_histories: Arc<RwLock<HashMap<String, HealthHistory>>>,
    alerts: Arc<RwLock<Vec<HealthAlert>>>,
    config: HealthMonitorConfig,
}

#[derive(Debug, Clone)]
pub struct HealthMonitorConfig {
    pub history_size: usize,
    pub alert_threshold_failures: u32,
    pub alert_threshold_response_time_ms: u64,
    pub alert_cooldown_minutes: u64,
    pub uptime_calculation_hours: i64,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            history_size: 1000,
            alert_threshold_failures: 3,
            alert_threshold_response_time_ms: 5000,
            alert_cooldown_minutes: 15,
            uptime_calculation_hours: 24,
        }
    }
}

impl HealthMonitor {
    pub fn new(registry: Arc<HealthCheckRegistry>, config: HealthMonitorConfig) -> Self {
        Self {
            registry,
            health_histories: Arc::new(RwLock::new(HashMap::new())),
            alerts: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    pub async fn run_health_checks(&self) -> Result<ServiceHealth> {
        let start_time = std::time::Instant::now();
        let results = self.registry.run_all_checks().await;
        let duration = start_time.elapsed().as_millis() as u64;

        let overall_status = self.calculate_overall_status(&results);
        let uptime = self.calculate_overall_uptime().await;

        let health = ServiceHealth {
            overall_status: overall_status.clone(),
            checks: results.clone(),
            timestamp: Utc::now(),
            uptime_percentage: uptime,
            last_check_duration_ms: duration,
        };

        // Update histories and check for alerts
        self.update_histories(&results).await;
        self.check_for_alerts(&results).await;

        info!(
            "Health check completed: status={:?}, duration={}ms, uptime={:.2}%",
            overall_status, duration, uptime
        );

        Ok(health)
    }

    async fn update_histories(&self, results: &[HealthCheckResult]) {
        let mut histories = self.health_histories.write().await;
        let now = Utc::now();

        for result in results {
            let history = histories
                .entry(result.service_name.clone())
                .or_insert_with(|| HealthHistory::new(result.service_name.clone(), self.config.history_size));
            
            history.add_result(now, result.status.clone());
        }
    }

    async fn check_for_alerts(&self, results: &[HealthCheckResult]) {
        let mut alerts = self.alerts.write().await;
        let now = Utc::now();

        for result in results {
            let service_name = &result.service_name;
            
            // Check if we should create an alert
            if self.should_create_alert(service_name, &result.status, &alerts, now).await {
                let alert_type = match result.status {
                    HealthStatus::Unhealthy => AlertType::ServiceDown,
                    HealthStatus::Degraded => AlertType::ServiceDegraded,
                    HealthStatus::Healthy => AlertType::ServiceRecovered,
                    HealthStatus::Unknown => continue,
                };

                let severity = match result.status {
                    HealthStatus::Unhealthy => AlertSeverity::Critical,
                    HealthStatus::Degraded => AlertSeverity::Warning,
                    HealthStatus::Healthy => AlertSeverity::Info,
                    HealthStatus::Unknown => AlertSeverity::Info,
                };

                let alert = HealthAlert::new(
                    service_name.clone(),
                    alert_type,
                    format!("Service {}: {:?}", service_name, result.status),
                    severity,
                );

                alerts.push(alert.clone());
                warn!("Health alert created: {:?}", alert);
            }

            // Check for high response time alerts
            if result.response_time_ms > self.config.alert_threshold_response_time_ms {
                let alert = HealthAlert::new(
                    service_name.clone(),
                    AlertType::HighResponseTime,
                    format!("High response time: {}ms", result.response_time_ms),
                    AlertSeverity::Warning,
                );

                alerts.push(alert.clone());
                warn!("Response time alert created: {:?}", alert);
            }
        }
    }

    async fn should_create_alert(
        &self,
        service_name: &str,
        status: &HealthStatus,
        alerts: &[HealthAlert],
        now: DateTime<Utc>,
    ) -> bool {
        // Don't create alerts for healthy status unless there was a previous failure
        if status.is_healthy() {
            return self.had_recent_failure(service_name, alerts, now).await;
        }

        // Check cooldown period
        let cooldown_cutoff = now - chrono::Duration::minutes(self.config.alert_cooldown_minutes as i64);
        let recent_alerts: Vec<_> = alerts
            .iter()
            .filter(|alert| {
                alert.service_name == service_name
                    && alert.timestamp >= cooldown_cutoff
                    && !alert.resolved
            })
            .collect();

        recent_alerts.is_empty()
    }

    async fn had_recent_failure(
        &self,
        service_name: &str,
        alerts: &[HealthAlert],
        now: DateTime<Utc>,
    ) -> bool {
        let recent_cutoff = now - chrono::Duration::minutes(self.config.alert_cooldown_minutes as i64);
        
        alerts
            .iter()
            .any(|alert| {
                alert.service_name == service_name
                    && alert.timestamp >= recent_cutoff
                    && !alert.resolved
                    && (matches!(alert.alert_type, AlertType::ServiceDown) || matches!(alert.alert_type, AlertType::ServiceDegraded))
            })
    }

    fn calculate_overall_status(&self, results: &[HealthCheckResult]) -> HealthStatus {
        if results.is_empty() {
            return HealthStatus::Unknown;
        }

        let unhealthy_count = results
            .iter()
            .filter(|r| r.status.is_unhealthy())
            .count();

        let degraded_count = results
            .iter()
            .filter(|r| r.status.is_degraded())
            .count();

        let total_count = results.len();

        if unhealthy_count > 0 {
            HealthStatus::Unhealthy
        } else if degraded_count > total_count / 2 {
            HealthStatus::Degraded
        } else if degraded_count > 0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }

    async fn calculate_overall_uptime(&self) -> f64 {
        let histories = self.health_histories.read().await;
        
        if histories.is_empty() {
            return 100.0;
        }

        let total_uptime: f64 = histories
            .values()
            .map(|history| history.calculate_uptime(self.config.uptime_calculation_hours))
            .sum();

        total_uptime / histories.len() as f64
    }

    pub async fn get_service_health(&self) -> Result<ServiceHealth> {
        self.run_health_checks().await
    }

    pub async fn get_service_history(&self, service_name: &str) -> Option<HealthHistory> {
        let histories = self.health_histories.read().await;
        histories.get(service_name).cloned()
    }

    pub async fn get_all_histories(&self) -> HashMap<String, HealthHistory> {
        let histories = self.health_histories.read().await;
        histories.clone()
    }

    pub async fn get_active_alerts(&self) -> Vec<HealthAlert> {
        let alerts = self.alerts.read().await;
        alerts
            .iter()
            .filter(|alert| !alert.resolved)
            .cloned()
            .collect()
    }

    pub async fn get_all_alerts(&self) -> Vec<HealthAlert> {
        let alerts = self.alerts.read().await;
        alerts.clone()
    }

    pub async fn resolve_alert(&self, alert_id: &str) -> Result<bool> {
        let mut alerts = self.alerts.write().await;
        
        for alert in alerts.iter_mut() {
            if alert.id == alert_id {
                alert.resolve();
                info!("Alert resolved: {}", alert_id);
                return Ok(true);
            }
        }
        
        Ok(false)
    }

    pub async fn get_health_summary(&self) -> Result<HealthSummary> {
        let health = self.get_service_health().await?;
        let histories = self.get_all_histories().await;
        let active_alerts = self.get_active_alerts().await;

        let mut service_summaries = HashMap::new();
        for (service_name, history) in histories {
            let current_status = history.get_current_status();
            let uptime = history.calculate_uptime(self.config.uptime_calculation_hours);
            let trend = history.get_status_trend();

            service_summaries.insert(
                service_name.clone(),
                ServiceSummary {
                    name: service_name,
                    current_status,
                    uptime_percentage: uptime,
                    trend,
                },
            );
        }

        Ok(HealthSummary {
            overall_status: health.overall_status,
            overall_uptime: health.uptime_percentage,
            service_summaries,
            active_alerts_count: active_alerts.len(),
            last_check: health.timestamp,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSummary {
    pub overall_status: HealthStatus,
    pub overall_uptime: f64,
    pub service_summaries: HashMap<String, ServiceSummary>,
    pub active_alerts_count: usize,
    pub last_check: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSummary {
    pub name: String,
    pub current_status: HealthStatus,
    pub uptime_percentage: f64,
    pub trend: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_health_history() {
        let mut history = HealthHistory::new("test".to_string(), 5);
        
        history.add_result(Utc::now(), HealthStatus::Healthy);
        history.add_result(Utc::now(), HealthStatus::Healthy);
        history.add_result(Utc::now(), HealthStatus::Degraded);
        
        assert_eq!(history.get_current_status(), HealthStatus::Degraded);
        assert!(history.calculate_uptime(24) > 0.0);
    }

    #[test]
    fn test_health_alert() {
        let mut alert = HealthAlert::new(
            "test".to_string(),
            AlertType::ServiceDown,
            "Service is down".to_string(),
            AlertSeverity::Critical,
        );
        
        assert!(!alert.resolved);
        assert!(alert.resolved_at.is_none());
        
        alert.resolve();
        assert!(alert.resolved);
        assert!(alert.resolved_at.is_some());
    }

    #[tokio::test]
    async fn test_health_monitor_config() {
        let config = HealthMonitorConfig::default();
        assert_eq!(config.history_size, 1000);
        assert_eq!(config.alert_threshold_failures, 3);
        assert_eq!(config.alert_threshold_response_time_ms, 5000);
    }
}
