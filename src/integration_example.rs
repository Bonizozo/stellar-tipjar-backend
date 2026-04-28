//! Integration example showing how to use all implemented features together.
//! This file demonstrates the recommended setup for production use.

use axum::Router;
use std::sync::Arc;
use std::time::Duration;

// Import all the implemented modules
use crate::security::replay_protection::{ReplayProtectionService, ReplayProtectionConfig};
use crate::security::session_management::{SessionManager, SessionConfig};
use crate::deduplication::{DeduplicationService, DeduplicationConfig};
use crate::health::{
    HealthCheckRegistry, HealthCheckConfig, HealthMonitor, HealthMonitorConfig,
    DatabaseHealthCheck, RedisHealthCheck, StellarHealthCheck,
    DiskSpaceHealthCheck, MemoryHealthCheck,
    RecoveryManager, RecoveryConfig, RecoveryPolicy, RecoveryActionType,
    DatabaseRecoveryHandler, RedisRecoveryHandler, ServiceRecoveryHandler,
    HealthDashboard
};
use crate::middleware::{
    replay_protection::{ReplayProtectionMiddlewareFactory, replay_protection_middleware},
    session::{SessionMiddlewareFactory, session_middleware, SessionMiddlewareState},
    deduplication::{DeduplicationMiddlewareFactory, deduplication_middleware, DeduplicationMiddlewareState},
};

/// Application state containing all security and monitoring services
pub struct ApplicationState {
    pub replay_protection: Arc<ReplayProtectionService>,
    pub session_manager: Arc<SessionManager>,
    pub deduplication_service: Arc<DeduplicationService>,
    pub health_monitor: Arc<HealthMonitor>,
    pub recovery_manager: Arc<RecoveryManager>,
    pub health_dashboard: Arc<HealthDashboard>,
}

impl ApplicationState {
    /// Initialize all services with production-ready configuration
    pub async fn new(
        pool: Arc<sqlx::postgres::PgPool>,
        redis: Option<Arc<redis::aio::ConnectionManager>>,
        stellar_rpc_url: String,
    ) -> Result<Self, anyhow::Error> {
        // Initialize Replay Protection Service
        let replay_config = ReplayProtectionConfig {
            nonce_ttl_seconds: 300, // 5 minutes
            max_timestamp_drift_seconds: 60, // 1 minute
            cleanup_interval_seconds: 600, // 10 minutes
            enabled_endpoints: vec![
                "/api/v1/tips".to_string(),
                "/api/v1/creators".to_string(),
                "/api/v1/withdrawals".to_string(),
                "/api/v1/transfers".to_string(),
            ],
        };
        let replay_protection = Arc::new(
            ReplayProtectionService::new(redis.clone(), replay_config)
        );

        // Initialize Session Manager
        let session_config = SessionConfig {
            session_ttl_seconds: 3600, // 1 hour
            idle_timeout_seconds: 1800, // 30 minutes
            absolute_timeout_seconds: 86400, // 24 hours
            cleanup_interval_seconds: 300, // 5 minutes
            max_sessions_per_user: 5,
            cookie_name: "stellar_session".to_string(),
            secure_cookies: true,
        };
        let session_manager = Arc::new(
            SessionManager::new(redis.clone(), session_config)
        );

        // Initialize Deduplication Service
        let deduplication_config = DeduplicationConfig {
            default_ttl_seconds: 300, // 5 minutes
            idempotent_ttl_seconds: 3600, // 1 hour
            cleanup_interval_seconds: 600, // 10 minutes
            max_stored_requests: 10000,
            enabled_endpoints: vec![
                "/api/v1/tips".to_string(),
                "/api/v1/withdrawals".to_string(),
                "/api/v1/transfers".to_string(),
            ],
            fingerprint_config: Default::default(),
        };
        let deduplication_service = Arc::new(
            DeduplicationService::new(redis.clone(), deduplication_config)
        );

        // Initialize Health Check Registry
        let mut health_registry = HealthCheckRegistry::new(HealthCheckConfig::default());
        
        // Add health checks
        health_registry.register_check(Box::new(
            DatabaseHealthCheck::new(pool.clone(), HealthCheckConfig::default())
        ));
        health_registry.register_check(Box::new(
            RedisHealthCheck::new(redis.clone(), HealthCheckConfig::default())
        ));
        health_registry.register_check(Box::new(
            StellarHealthCheck::new(stellar_rpc_url, HealthCheckConfig::default())
        ));
        health_registry.register_check(Box::new(
            DiskSpaceHealthCheck::new(
                HealthCheckConfig::default(),
                10_000_000_000, // 10GB warning
                5_000_000_000,  // 5GB critical
            )
        ));
        health_registry.register_check(Box::new(
            MemoryHealthCheck::new(
                HealthCheckConfig::default(),
                80.0, // 80% warning
                95.0, // 95% critical
            )
        ));

        // Initialize Health Monitor
        let health_monitor = Arc::new(
            HealthMonitor::new(
                Arc::new(health_registry),
                HealthMonitorConfig::default()
            )
        );

        // Initialize Recovery Manager
        let recovery_config = RecoveryConfig::default();
        let mut recovery_manager = RecoveryManager::new(recovery_config);

        // Add recovery handlers
        recovery_manager.register_handler(Box::new(
            DatabaseRecoveryHandler::new("database".to_string())
        ));
        recovery_manager.register_handler(Box::new(
            RedisRecoveryHandler::new("redis".to_string())
        ));
        recovery_manager.register_handler(Box::new(
            ServiceRecoveryHandler::new("api".to_string())
        ));

        // Add recovery policies
        recovery_manager.add_policy(
            RecoveryPolicy::new("database".to_string())
                .with_actions(vec![
                    RecoveryActionType::ReconnectDatabase,
                    RecoveryActionType::RestartService,
                ])
                .with_threshold(3)
        );
        recovery_manager.add_policy(
            RecoveryPolicy::new("redis".to_string())
                .with_actions(vec![
                    RecoveryActionType::ReconnectRedis,
                    RecoveryActionType::ClearCache,
                ])
                .with_threshold(2)
        );
        recovery_manager.add_policy(
            RecoveryPolicy::new("stellar".to_string())
                .with_actions(vec![
                    RecoveryActionType::RestartService,
                ])
                .with_threshold(5)
        );

        // Initialize Health Dashboard
        let health_dashboard = Arc::new(
            HealthDashboard::new(
                health_monitor.clone(),
                recovery_manager.clone()
            )?
        );

        Ok(Self {
            replay_protection,
            session_manager,
            deduplication_service,
            health_monitor,
            recovery_manager,
            health_dashboard,
        })
    }

    /// Create an Axum router with all middleware layers applied
    pub fn create_router(&self, api_router: Router) -> Router {
        api_router
            // Apply middleware in order (outermost first)
            .layer(axum::middleware::from_fn_with_state(
                self.replay_protection.clone(),
                replay_protection_middleware
            ))
            .layer(axum::middleware::from_fn_with_state(
                SessionMiddlewareState { session_manager: self.session_manager.clone() },
                session_middleware
            ))
            .layer(axum::middleware::from_fn_with_state(
                DeduplicationMiddlewareState { deduplication_service: self.deduplication_service.clone() },
                deduplication_middleware
            ))
    }

    /// Start background tasks for health monitoring and cleanup
    pub async fn start_background_tasks(&self) -> Result<(), anyhow::Error> {
        // Clone services for background tasks
        let health_monitor = self.health_monitor.clone();
        let recovery_manager = self.recovery_manager.clone();
        let replay_protection = self.replay_protection.clone();
        let session_manager = self.session_manager.clone();
        let deduplication_service = self.deduplication_service.clone();

        // Health monitoring task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                match health_monitor.run_health_checks().await {
                    Ok(health) => {
                        tracing::info!(
                            "Health check completed: status={:?}, uptime={:.2}%",
                            health.overall_status,
                            health.uptime_percentage
                        );

                        // Trigger recovery if needed
                        for check_result in &health.checks {
                            if check_result.status.is_unhealthy() || check_result.status.is_degraded() {
                                match recovery_manager.handle_health_failure(
                                    &check_result.service_name,
                                    check_result
                                ).await {
                                    Ok(actions) => {
                                        if !actions.is_empty() {
                                            tracing::info!(
                                                "Executed {} recovery actions for service '{}'",
                                                actions.len(),
                                                check_result.service_name
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Recovery failed for service '{}': {}",
                                            check_result.service_name,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Health check failed: {}", e);
                    }
                }
            }
        });

        // Cleanup tasks
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600)); // Every 10 minutes
            
            loop {
                interval.tick().await;
                
                // Cleanup expired nonces
                if let Ok(count) = replay_protection.cleanup_expired_nonces().await {
                    if count > 0 {
                        tracing::info!("Cleaned up {} expired nonces", count);
                    }
                }

                // Cleanup expired sessions
                if let Ok(count) = session_manager.cleanup_expired_sessions().await {
                    if count > 0 {
                        tracing::info!("Cleaned up {} expired sessions", count);
                    }
                }

                // Cleanup expired deduplication records
                if let Ok(count) = deduplication_service.cleanup_expired_records().await {
                    if count > 0 {
                        tracing::info!("Cleaned up {} expired deduplication records", count);
                    }
                }
            }
        });

        Ok(())
    }
}

/// Example of how to use the integrated system
pub async fn example_usage() -> Result<(), anyhow::Error> {
    // This would be called from your main.rs after setting up database and Redis
    
    /*
    // Initialize application state
    let app_state = ApplicationState::new(
        pool,
        redis,
        stellar_rpc_url,
    ).await?;

    // Create API router (your existing routes)
    let api_router = Router::new()
        .route("/api/v1/tips", axum::routing::post(create_tip))
        .route("/api/v1/creators", axum::routing::post(create_creator))
        .route("/api/v1/health", axum::routing::get(health_check))
        .route("/api/v1/health/dashboard", axum::routing::get(health_dashboard_handler))
        .with_state(app_state.clone());

    // Apply all middleware layers
    let app = app_state.create_router(api_router);

    // Start background tasks
    app_state.start_background_tasks().await?;

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    tracing::info!("Server starting on 0.0.0.0:8000");
    axum::serve(listener, app).await?;
    */

    Ok(())
}

/// Example endpoint showing how to use the services manually
pub async fn example_create_tip_endpoint(
    axum::extract::State(state): axum::extract::State<Arc<ApplicationState>>,
    axum::extract::Json(payload): axum::extract::Json<serde_json::Value>,
) -> Result<axum::response::Json<serde_json::Value>, axum::response::Response> {
    // The middleware has already handled:
    // - Replay protection (nonce validation)
    // - Session authentication
    // - Request deduplication
    
    // You can access the services directly if needed
    let session = axum::extract::Extension::<crate::middleware::session::SessionData>::from_request(
        &axum::extract::Request::default(),
        &mut std::convert::Infallible::default(),
    ).await
        .map_err(|_| axum::response::Response::builder()
            .status(axum::http::StatusCode::UNAUTHORIZED)
            .body(axum::body::Body::from("Unauthorized"))
            .unwrap())?;

    tracing::info!("Processing tip request for user: {}", session.user_id);

    // Your business logic here
    let response = serde_json::json!({
        "success": true,
        "tip_id": uuid::Uuid::new_v4().to_string(),
        "user_id": session.user_id,
        "processed_at": chrono::Utc::now(),
    });

    Ok(axum::response::Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_application_state_creation() {
        // This would require setting up test database and Redis
        // In a real test, you'd use in-memory or test containers
    }

    #[tokio::test]
    async fn test_background_task_simulation() {
        // Test that background tasks can be started
        // This would require mock services in a real test
    }
}
