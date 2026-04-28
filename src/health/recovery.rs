use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use super::checks::{HealthCheckRegistry, HealthCheckResult, HealthStatus};
use super::monitoring::{HealthAlert, AlertType, AlertSeverity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    pub id: String,
    pub service_name: String,
    pub action_type: RecoveryActionType,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub result: Option<RecoveryResult>,
    pub automatic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryActionType {
    RestartService,
    ClearCache,
    ReconnectDatabase,
    ReconnectRedis,
    ScaleUp,
    ScaleDown,
    FlushQueue,
    ResetMetrics,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryResult {
    Success(String),
    Failed(String),
    Pending,
}

impl RecoveryAction {
    pub fn new(
        service_name: String,
        action_type: RecoveryActionType,
        description: String,
        automatic: bool,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            service_name,
            action_type,
            description,
            created_at: Utc::now(),
            executed_at: None,
            result: None,
            automatic,
        }
    }

    pub fn execute(&mut self, result: RecoveryResult) {
        self.executed_at = Some(Utc::now());
        self.result = Some(result);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    pub service_name: String,
    pub failure_threshold: u32,
    pub recovery_actions: Vec<RecoveryActionType>,
    pub cooldown_minutes: u64,
    pub max_attempts_per_hour: u32,
    pub enabled: bool,
}

impl RecoveryPolicy {
    pub fn new(service_name: String) -> Self {
        Self {
            service_name,
            failure_threshold: 3,
            recovery_actions: vec![
                RecoveryActionType::ReconnectDatabase,
                RecoveryActionType::ClearCache,
                RecoveryActionType::RestartService,
            ],
            cooldown_minutes: 15,
            max_attempts_per_hour: 5,
            enabled: true,
        }
    }

    pub fn with_actions(mut self, actions: Vec<RecoveryActionType>) -> Self {
        self.recovery_actions = actions;
        self
    }

    pub fn with_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }
}

pub trait RecoveryHandler: Send + Sync {
    async fn execute_action(&self, action: &RecoveryActionType) -> Result<RecoveryResult>;
    fn service_name(&self) -> &str;
}

pub struct DatabaseRecoveryHandler {
    service_name: String,
}

impl DatabaseRecoveryHandler {
    pub fn new(service_name: String) -> Self {
        Self { service_name }
    }
}

impl RecoveryHandler for DatabaseRecoveryHandler {
    async fn execute_action(&self, action: &RecoveryActionType) -> Result<RecoveryResult> {
        match action {
            RecoveryActionType::ReconnectDatabase => {
                info!("Attempting to reconnect database for service: {}", self.service_name);
                
                // Simulate reconnection attempt
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                
                // In a real implementation, you would:
                // 1. Close existing connections
                // 2. Test connectivity
                // 3. Re-establish connection pool
                
                Ok(RecoveryResult::Success("Database reconnected successfully".to_string()))
            }
            _ => Ok(RecoveryResult::Failed("Action not supported for database".to_string())),
        }
    }

    fn service_name(&self) -> &str {
        &self.service_name
    }
}

pub struct RedisRecoveryHandler {
    service_name: String,
}

impl RedisRecoveryHandler {
    pub fn new(service_name: String) -> Self {
        Self { service_name }
    }
}

impl RecoveryHandler for RedisRecoveryHandler {
    async fn execute_action(&self, action: &RecoveryActionType) -> Result<RecoveryResult> {
        match action {
            RecoveryActionType::ReconnectRedis => {
                info!("Attempting to reconnect Redis for service: {}", self.service_name);
                
                // Simulate reconnection attempt
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                // In a real implementation, you would:
                // 1. Close existing Redis connections
                // 2. Test connectivity
                // 3. Re-establish Redis connection
                
                Ok(RecoveryResult::Success("Redis reconnected successfully".to_string()))
            }
            RecoveryActionType::ClearCache => {
                info!("Clearing Redis cache for service: {}", self.service_name);
                
                // Simulate cache clearing
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                
                // In a real implementation, you would:
                // 1. Flush Redis cache
                // 2. Clear local cache
                // 3. Invalidate cache entries
                
                Ok(RecoveryResult::Success("Cache cleared successfully".to_string()))
            }
            _ => Ok(RecoveryResult::Failed("Action not supported for Redis".to_string())),
        }
    }

    fn service_name(&self) -> &str {
        &self.service_name
    }
}

pub struct ServiceRecoveryHandler {
    service_name: String,
}

impl ServiceRecoveryHandler {
    pub fn new(service_name: String) -> Self {
        Self { service_name }
    }
}

impl RecoveryHandler for ServiceRecoveryHandler {
    async fn execute_action(&self, action: &RecoveryActionType) -> Result<RecoveryResult> {
        match action {
            RecoveryActionType::RestartService => {
                info!("Attempting to restart service: {}", self.service_name);
                
                // Simulate service restart
                tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
                
                // In a real implementation, you would:
                // 1. Gracefully shutdown the service
                // 2. Wait for shutdown completion
                // 3. Start the service
                // 4. Verify health
                
                Ok(RecoveryResult::Success("Service restarted successfully".to_string()))
            }
            RecoveryActionType::FlushQueue => {
                info!("Flushing message queue for service: {}", self.service_name);
                
                // Simulate queue flush
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                
                Ok(RecoveryResult::Success("Queue flushed successfully".to_string()))
            }
            RecoveryActionType::ResetMetrics => {
                info!("Resetting metrics for service: {}", self.service_name);
                
                // Simulate metrics reset
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                
                Ok(RecoveryResult::Success("Metrics reset successfully".to_string()))
            }
            _ => Ok(RecoveryResult::Failed("Action not supported for this service".to_string())),
        }
    }

    fn service_name(&self) -> &str {
        &self.service_name
    }
}

pub struct RecoveryManager {
    handlers: HashMap<String, Box<dyn RecoveryHandler>>,
    policies: HashMap<String, RecoveryPolicy>,
    actions: Arc<RwLock<Vec<RecoveryAction>>>,
    config: RecoveryConfig,
}

#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    pub max_concurrent_actions: usize,
    pub action_timeout_seconds: u64,
    pub retry_attempts: u32,
    pub retry_delay_ms: u64,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_concurrent_actions: 3,
            action_timeout_seconds: 300,
            retry_attempts: 3,
            retry_delay_ms: 1000,
        }
    }
}

impl RecoveryManager {
    pub fn new(config: RecoveryConfig) -> Self {
        Self {
            handlers: HashMap::new(),
            policies: HashMap::new(),
            actions: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    pub fn register_handler(&mut self, handler: Box<dyn RecoveryHandler>) {
        let service_name = handler.service_name().to_string();
        self.handlers.insert(service_name, handler);
    }

    pub fn add_policy(&mut self, policy: RecoveryPolicy) {
        self.policies.insert(policy.service_name.clone(), policy);
    }

    pub async fn handle_health_failure(&self, service_name: &str, health_result: &HealthCheckResult) -> Result<Vec<RecoveryAction>> {
        if health_result.status.is_healthy() {
            return Ok(Vec::new());
        }

        let policy = match self.policies.get(service_name) {
            Some(policy) if policy.enabled => policy,
            _ => return Ok(Vec::new()),
        };

        // Check if we should trigger recovery
        if !self.should_trigger_recovery(service_name, policy).await {
            return Ok(Vec::new());
        }

        let mut executed_actions = Vec::new();

        for action_type in &policy.recovery_actions {
            if let Some(handler) = self.handlers.get(service_name) {
                let mut action = RecoveryAction::new(
                    service_name.to_string(),
                    action_type.clone(),
                    format!("Automatic recovery: {:?}", action_type),
                    true,
                );

                info!("Executing recovery action: {:?} for service: {}", action_type, service_name);

                let result = tokio::time::timeout(
                    tokio::time::Duration::from_secs(self.config.action_timeout_seconds),
                    self.execute_action_with_retry(handler, action_type)
                ).await;

                match result {
                    Ok(Ok(recovery_result)) => {
                        action.execute(recovery_result);
                        info!("Recovery action completed successfully: {:?}", action_type);
                    }
                    Ok(Err(e)) => {
                        action.execute(RecoveryResult::Failed(format!("Execution failed: {}", e)));
                        error!("Recovery action failed: {:?} - {}", action_type, e);
                    }
                    Err(_) => {
                        action.execute(RecoveryResult::Failed("Action timed out".to_string()));
                        error!("Recovery action timed out: {:?}", action_type);
                    }
                }

                executed_actions.push(action.clone());
                self.store_action(action).await;

                // If the action succeeded, we might not need to try further actions
                if let Some(RecoveryResult::Success(_)) = &executed_actions.last().unwrap().result {
                    break;
                }
            } else {
                warn!("No recovery handler found for service: {}", service_name);
            }
        }

        Ok(executed_actions)
    }

    async fn execute_action_with_retry(
        &self,
        handler: &Box<dyn RecoveryHandler>,
        action_type: &RecoveryActionType,
    ) -> Result<RecoveryResult> {
        let mut last_error = None;

        for attempt in 1..=self.config.retry_attempts {
            match handler.execute_action(action_type).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.retry_attempts {
                        warn!("Recovery action attempt {} failed, retrying in {}ms: {:?}", attempt, self.config.retry_delay_ms, action_type);
                        tokio::time::sleep(tokio::time::Duration::from_millis(self.config.retry_delay_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retry attempts failed")))
    }

    async fn should_trigger_recovery(&self, service_name: &str, policy: &RecoveryPolicy) -> bool {
        let actions = self.actions.read().await;
        let now = Utc::now();
        
        // Check cooldown period
        let cooldown_cutoff = now - chrono::Duration::minutes(policy.cooldown_minutes as i64);
        let recent_actions: Vec<_> = actions
            .iter()
            .filter(|action| {
                action.service_name == service_name
                    && action.created_at >= cooldown_cutoff
                    && action.result.is_some()
            })
            .collect();

        if recent_actions.len() as u32 >= policy.max_attempts_per_hour {
            info!("Recovery cooldown active for service: {}", service_name);
            return false;
        }

        // Check failure threshold
        let failed_actions = recent_actions
            .iter()
            .filter(|action| {
                matches!(action.result, Some(RecoveryResult::Failed(_)))
            })
            .count();

        failed_actions >= policy.failure_threshold as usize
    }

    async fn store_action(&self, action: RecoveryAction) {
        let mut actions = self.actions.write().await;
        actions.push(action);
        
        // Keep only recent actions (last 1000)
        if actions.len() > 1000 {
            actions.drain(0..actions.len() - 1000);
        }
    }

    pub async fn get_recovery_actions(&self, service_name: Option<&str>) -> Vec<RecoveryAction> {
        let actions = self.actions.read().await;
        
        if let Some(service_name) = service_name {
            actions
                .iter()
                .filter(|action| action.service_name == service_name)
                .cloned()
                .collect()
        } else {
            actions.clone()
        }
    }

    pub async fn get_recovery_stats(&self) -> RecoveryStats {
        let actions = self.actions.read().await;
        let now = Utc::now();
        let last_24h = now - chrono::Duration::hours(24);

        let total_actions = actions.len();
        let successful_actions = actions
            .iter()
            .filter(|action| matches!(action.result, Some(RecoveryResult::Success(_))))
            .count();
        
        let failed_actions = actions
            .iter()
            .filter(|action| matches!(action.result, Some(RecoveryResult::Failed(_))))
            .count();

        let recent_actions = actions
            .iter()
            .filter(|action| action.created_at >= last_24h)
            .count();

        let automatic_actions = actions
            .iter()
            .filter(|action| action.automatic)
            .count();

        let mut service_stats = HashMap::new();
        for action in actions.iter() {
            let stats = service_stats.entry(action.service_name.clone()).or_insert((0, 0, 0));
            stats.0 += 1; // total
            if matches!(action.result, Some(RecoveryResult::Success(_))) {
                stats.1 += 1; // successful
            }
            if action.automatic {
                stats.2 += 1; // automatic
            }
        }

        RecoveryStats {
            total_actions,
            successful_actions,
            failed_actions,
            recent_actions_24h: recent_actions,
            automatic_actions,
            service_stats,
        }
    }

    pub async fn execute_manual_recovery(
        &self,
        service_name: &str,
        action_type: RecoveryActionType,
    ) -> Result<RecoveryAction> {
        if let Some(handler) = self.handlers.get(service_name) {
            let mut action = RecoveryAction::new(
                service_name.to_string(),
                action_type.clone(),
                format!("Manual recovery: {:?}", action_type),
                false,
            );

            info!("Executing manual recovery action: {:?} for service: {}", action_type, service_name);

            let result = tokio::time::timeout(
                tokio::time::Duration::from_secs(self.config.action_timeout_seconds),
                self.execute_action_with_retry(handler, &action_type)
            ).await;

            match result {
                Ok(Ok(recovery_result)) => {
                    action.execute(recovery_result);
                    info!("Manual recovery action completed successfully: {:?}", action_type);
                }
                Ok(Err(e)) => {
                    action.execute(RecoveryResult::Failed(format!("Execution failed: {}", e)));
                    error!("Manual recovery action failed: {:?} - {}", action_type, e);
                }
                Err(_) => {
                    action.execute(RecoveryResult::Failed("Action timed out".to_string()));
                    error!("Manual recovery action timed out: {:?}", action_type);
                }
            }

            let action_clone = action.clone();
            self.store_action(action).await;

            Ok(action_clone)
        } else {
            Err(anyhow::anyhow!("No recovery handler found for service: {}", service_name))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryStats {
    pub total_actions: usize,
    pub successful_actions: usize,
    pub failed_actions: usize,
    pub recent_actions_24h: usize,
    pub automatic_actions: usize,
    pub service_stats: HashMap<String, (usize, usize, usize)>, // (total, successful, automatic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_recovery_action() {
        let mut action = RecoveryAction::new(
            "test".to_string(),
            RecoveryActionType::RestartService,
            "Test action".to_string(),
            true,
        );

        assert!(action.executed_at.is_none());
        assert!(action.result.is_none());

        action.execute(RecoveryResult::Success("Test success".to_string()));
        assert!(action.executed_at.is_some());
        assert!(action.result.is_some());
    }

    #[test]
    fn test_recovery_policy() {
        let policy = RecoveryPolicy::new("test".to_string())
            .with_actions(vec![RecoveryActionType::RestartService])
            .with_threshold(5);

        assert_eq!(policy.service_name, "test");
        assert_eq!(policy.failure_threshold, 5);
        assert_eq!(policy.recovery_actions.len(), 1);
    }

    #[tokio::test]
    async fn test_recovery_manager() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);

        // Add a test handler
        let handler = Box::new(ServiceRecoveryHandler::new("test".to_string()));
        manager.register_handler(handler);

        // Add a policy
        let policy = RecoveryPolicy::new("test".to_string());
        manager.add_policy(policy);

        let stats = manager.get_recovery_stats().await;
        assert_eq!(stats.total_actions, 0);
    }
}
