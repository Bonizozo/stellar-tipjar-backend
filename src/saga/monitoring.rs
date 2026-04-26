use super::workflow::{SagaStepStatus, SagaWorkflow};
use crate::errors::AppError;
use sqlx::PgPool;
use uuid::Uuid;

pub struct SagaMonitor {
    pool: PgPool,
}

impl SagaMonitor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record_step_execution(
        &self,
        workflow_id: Uuid,
        step_id: &str,
        status: &SagaStepStatus,
        duration_ms: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO saga_step_executions (workflow_id, step_id, status, duration_ms, executed_at)
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(workflow_id)
        .bind(step_id)
        .bind(format!("{:?}", status))
        .bind(duration_ms)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_workflow_metrics(
        &self,
        workflow_id: Uuid,
    ) -> Result<WorkflowMetrics, AppError> {
        let metrics = sqlx::query_as::<_, WorkflowMetrics>(
            "SELECT 
                $1 as workflow_id,
                COUNT(*) as total_steps,
                SUM(CASE WHEN status = 'Completed' THEN 1 ELSE 0 END) as completed_steps,
                SUM(CASE WHEN status = 'Failed' THEN 1 ELSE 0 END) as failed_steps,
                AVG(duration_ms) as avg_step_duration_ms,
                MAX(duration_ms) as max_step_duration_ms
             FROM saga_step_executions
             WHERE workflow_id = $1",
        )
        .bind(workflow_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(metrics)
    }

    pub async fn get_failed_workflows(&self, limit: i32) -> Result<Vec<FailedWorkflow>, AppError> {
        let workflows = sqlx::query_as::<_, FailedWorkflow>(
            "SELECT id, name, status, error_message, created_at
             FROM saga_workflows
             WHERE status = 'Failed'
             ORDER BY created_at DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(workflows)
    }

    pub async fn get_status_counts(&self) -> Result<Vec<SagaStatusCount>, AppError> {
        let rows = sqlx::query_as::<_, SagaStatusCount>(
            "SELECT status, COUNT(*)::BIGINT AS total
             FROM saga_workflows
             GROUP BY status
             ORDER BY total DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkflowMetrics {
    pub workflow_id: Uuid,
    pub total_steps: i64,
    pub completed_steps: i64,
    pub failed_steps: i64,
    pub avg_step_duration_ms: Option<f64>,
    pub max_step_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FailedWorkflow {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SagaStatusCount {
    pub status: String,
    pub total: i64,
}
