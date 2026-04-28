use axum::extract::{State, Query};
use axum::response::{Html, IntoResponse, Response};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tera::{Tera, Context};

use super::monitoring::{HealthMonitor, HealthSummary};
use super::recovery::{RecoveryManager, RecoveryAction, RecoveryActionType};

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    pub refresh: Option<u64>,
    pub service: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DashboardData {
    pub health_summary: HealthSummary,
    pub recovery_actions: Vec<RecoveryAction>,
    pub refresh_interval: u64,
    pub selected_service: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub struct HealthDashboard {
    monitor: Arc<HealthMonitor>,
    recovery_manager: Arc<RecoveryManager>,
    templates: Tera,
}

impl HealthDashboard {
    pub fn new(
        monitor: Arc<HealthMonitor>,
        recovery_manager: Arc<RecoveryManager>,
    ) -> Result<Self, tera::Error> {
        let mut templates = Tera::default();
        
        // Add inline templates
        templates.add_raw_template(
            "dashboard.html",
            include_str!("templates/dashboard.html"),
        )?;
        
        templates.add_raw_template(
            "health_table.html",
            include_str!("templates/health_table.html"),
        )?;

        Ok(Self {
            monitor,
            recovery_manager,
            templates,
        })
    }

    pub async fn render_dashboard(
        &self,
        query: DashboardQuery,
    ) -> Result<Html<String>, anyhow::Error> {
        let health_summary = self.monitor.get_health_summary().await?;
        let recovery_actions = self.recovery_manager.get_recovery_actions(query.service.as_deref()).await;

        let refresh_interval = query.refresh.unwrap_or(30);

        let data = DashboardData {
            health_summary,
            recovery_actions,
            refresh_interval,
            selected_service: query.service,
            timestamp: chrono::Utc::now(),
        };

        let mut context = Context::new();
        context.insert("data", &data);

        let rendered = self.templates.render("dashboard.html", &context)?;
        Ok(Html(rendered))
    }

    pub async fn render_health_table(&self) -> Result<Html<String>, anyhow::Error> {
        let health = self.monitor.get_service_health().await?;

        let mut context = Context::new();
        context.insert("health", &health);

        let rendered = self.templates.render("health_table.html", &context)?;
        Ok(Html(rendered))
    }

    pub async fn execute_manual_recovery(
        &self,
        service_name: &str,
        action_type: &str,
    ) -> Result<Response, anyhow::Error> {
        let action_type = match action_type {
            "restart_service" => RecoveryActionType::RestartService,
            "clear_cache" => RecoveryActionType::ClearCache,
            "reconnect_database" => RecoveryActionType::ReconnectDatabase,
            "reconnect_redis" => RecoveryActionType::ReconnectRedis,
            "flush_queue" => RecoveryActionType::FlushQueue,
            "reset_metrics" => RecoveryActionType::ResetMetrics,
            _ => return Ok((StatusCode::BAD_REQUEST, "Invalid action type").into_response()),
        };

        match self.recovery_manager.execute_manual_recovery(service_name, action_type).await {
            Ok(action) => {
                let body = serde_json::json!({
                    "success": true,
                    "action": action,
                    "message": format!("Recovery action '{:?}' executed for service '{}'", action_type, service_name)
                });
                Ok((StatusCode::OK, axum::Json(body)).into_response())
            }
            Err(e) => {
                let body = serde_json::json!({
                    "success": false,
                    "error": e.to_string(),
                    "message": format!("Failed to execute recovery action for service '{}': {}", service_name, e)
                });
                Ok((StatusCode::INTERNAL_SERVER_ERROR, axum::Json(body)).into_response())
            }
        }
    }
}

// HTML templates (normally these would be in separate files)
pub const DASHBOARD_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Service Health Dashboard</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            margin: 0;
            padding: 20px;
            background-color: #f5f5f5;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
        }
        .header {
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            margin-bottom: 20px;
        }
        .status-overview {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-bottom: 20px;
        }
        .status-card {
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            text-align: center;
        }
        .status-healthy { border-left: 4px solid #28a745; }
        .status-degraded { border-left: 4px solid #ffc107; }
        .status-unhealthy { border-left: 4px solid #dc3545; }
        .status-unknown { border-left: 4px solid #6c757d; }
        
        .services-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
            margin-bottom: 20px;
        }
        .service-card {
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }
        .service-header {
            display: flex;
            justify-content: between;
            align-items: center;
            margin-bottom: 15px;
        }
        .service-name {
            font-weight: bold;
            font-size: 1.1em;
        }
        .service-status {
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 0.8em;
            color: white;
        }
        .status-healthy-badge { background-color: #28a745; }
        .status-degraded-badge { background-color: #ffc107; }
        .status-unhealthy-badge { background-color: #dc3545; }
        .status-unknown-badge { background-color: #6c757d; }
        
        .recovery-actions {
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }
        .action-item {
            padding: 10px;
            border-left: 3px solid #007bff;
            margin-bottom: 10px;
            background: #f8f9fa;
        }
        .action-success { border-left-color: #28a745; }
        .action-failed { border-left-color: #dc3545; }
        
        .btn {
            background: #007bff;
            color: white;
            border: none;
            padding: 8px 16px;
            border-radius: 4px;
            cursor: pointer;
            margin-right: 10px;
        }
        .btn:hover { background: #0056b3; }
        .btn-danger { background: #dc3545; }
        .btn-danger:hover { background: #c82333; }
        
        .refresh-info {
            text-align: center;
            color: #6c757d;
            margin-top: 20px;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Service Health Dashboard</h1>
            <p>Last updated: {{ data.timestamp }}</p>
            <p>Auto-refresh: {{ data.refresh_interval }}s</p>
        </div>

        <div class="status-overview">
            <div class="status-card status-{{ data.health_summary.overall_status | lower }}">
                <h3>Overall Status</h3>
                <div class="service-status status-{{ data.health_summary.overall_status | lower }}-badge">
                    {{ data.health_summary.overall_status }}
                </div>
            </div>
            <div class="status-card">
                <h3>Overall Uptime</h3>
                <div>{{ "%.2f"|format(data.health_summary.overall_uptime) }}%</div>
            </div>
            <div class="status-card">
                <h3>Active Alerts</h3>
                <div>{{ data.health_summary.active_alerts_count }}</div>
            </div>
            <div class="status-card">
                <h3>Services</h3>
                <div>{{ data.health_summary.service_summaries | length }}</div>
            </div>
        </div>

        <div class="services-grid">
            {% for service_name, service_summary in data.health_summary.service_summaries %}
            <div class="service-card">
                <div class="service-header">
                    <div class="service-name">{{ service_name }}</div>
                    <div class="service-status status-{{ service_summary.current_status | lower }}-badge">
                        {{ service_summary.current_status }}
                    </div>
                </div>
                <div><strong>Uptime:</strong> {{ "%.2f"|format(service_summary.uptime_percentage) }}%</div>
                <div><strong>Trend:</strong> {{ service_summary.trend }}</div>
                <div style="margin-top: 15px;">
                    <button class="btn" onclick="executeRecovery('{{ service_name }}', 'restart_service')">Restart</button>
                    <button class="btn" onclick="executeRecovery('{{ service_name }}', 'clear_cache')">Clear Cache</button>
                </div>
            </div>
            {% endfor %}
        </div>

        {% if data.recovery_actions | length > 0 %}
        <div class="recovery-actions">
            <h3>Recent Recovery Actions</h3>
            {% for action in data.recovery_actions | slice(end=10) %}
            <div class="action-item {% if action.result.Success %}action-success{% elif action.result.Failed %}action-failed{% endif %}">
                <div><strong>{{ action.service_name }}</strong> - {{ action.action_type }}</div>
                <div>{{ action.description }}</div>
                <div style="font-size: 0.8em; color: #6c757d;">
                    {{ action.created_at }} 
                    {% if action.executed_at %}• {{ action.executed_at }}{% endif %}
                    {% if action.result %}• {{ action.result }}{% endif %}
                </div>
            </div>
            {% endfor %}
        </div>
        {% endif %}

        <div class="refresh-info">
            Page refreshes every {{ data.refresh_interval }} seconds
        </div>
    </div>

    <script>
        function executeRecovery(serviceName, actionType) {
            if (confirm(`Execute recovery action '${actionType}' for service '${serviceName}'?`)) {
                fetch(`/api/v1/health/recovery`, {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({
                        service_name: serviceName,
                        action_type: actionType
                    })
                })
                .then(response => response.json())
                .then(data => {
                    if (data.success) {
                        alert('Recovery action executed successfully!');
                        location.reload();
                    } else {
                        alert('Failed to execute recovery action: ' + data.error);
                    }
                })
                .catch(error => {
                    alert('Error executing recovery action: ' + error);
                });
            }
        }

        // Auto-refresh
        setTimeout(() => {
            location.reload();
        }, {{ data.refresh_interval }} * 1000);
    </script>
</body>
</html>
"#;

pub const HEALTH_TABLE_TEMPLATE: &str = r#"
<table style="width: 100%; border-collapse: collapse;">
    <thead>
        <tr style="background: #f8f9fa;">
            <th style="padding: 10px; border: 1px solid #dee2e6;">Service</th>
            <th style="padding: 10px; border: 1px solid #dee2e6;">Status</th>
            <th style="padding: 10px; border: 1px solid #dee2e6;">Response Time</th>
            <th style="padding: 10px; border: 1px solid #dee2e6;">Message</th>
            <th style="padding: 10px; border: 1px solid #dee2e6;">Last Check</th>
        </tr>
    </thead>
    <tbody>
        {% for check in health.checks %}
        <tr>
            <td style="padding: 10px; border: 1px solid #dee2e6;">{{ check.service_name }}</td>
            <td style="padding: 10px; border: 1px solid #dee2e6;">
                <span class="service-status status-{{ check.status | lower }}-badge">
                    {{ check.status }}
                </span>
            </td>
            <td style="padding: 10px; border: 1px solid #dee2e6;">{{ check.response_time_ms }}ms</td>
            <td style="padding: 10px; border: 1px solid #dee2e6;">{{ check.message }}</td>
            <td style="padding: 10px; border: 1px solid #dee2e6;">{{ check.timestamp }}</td>
        </tr>
        {% endfor %}
    </tbody>
</table>
"#;

// Route handlers for the health dashboard
pub async fn health_dashboard_handler(
    State(dashboard): State<Arc<HealthDashboard>>,
    Query(query): Query<DashboardQuery>,
) -> impl IntoResponse {
    match dashboard.render_dashboard(query).await {
        Ok(html) => html.into_response(),
        Err(e) => {
            tracing::error!("Failed to render health dashboard: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        }
    }
}

pub async fn health_table_handler(
    State(dashboard): State<Arc<HealthDashboard>>,
) -> impl IntoResponse {
    match dashboard.render_health_table().await {
        Ok(html) => html.into_response(),
        Err(e) => {
            tracing::error!("Failed to render health table: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct RecoveryRequest {
    pub service_name: String,
    pub action_type: String,
}

pub async fn recovery_action_handler(
    State(dashboard): State<Arc<HealthDashboard>>,
    axum::Json(payload): axum::Json<RecoveryRequest>,
) -> impl IntoResponse {
    dashboard.execute_manual_recovery(&payload.service_name, &payload.action_type).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_dashboard_query() {
        let query = DashboardQuery {
            refresh: Some(60),
            service: Some("test".to_string()),
        };
        
        assert_eq!(query.refresh, Some(60));
        assert_eq!(query.service, Some("test".to_string()));
    }

    #[test]
    fn test_recovery_request() {
        let request = RecoveryRequest {
            service_name: "test".to_string(),
            action_type: "restart_service".to_string(),
        };
        
        assert_eq!(request.service_name, "test");
        assert_eq!(request.action_type, "restart_service");
    }
}
