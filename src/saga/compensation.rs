use crate::errors::AppError;
use sqlx::PgPool;
use uuid::Uuid;
use super::workflow::{SagaWorkflow, SagaStepStatus};

pub struct CompensationHandler {
    pool: PgPool,
}

impl CompensationHandler {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn compensate_workflow(&self, workflow: &mut SagaWorkflow) -> Result<(), AppError> {
        workflow.status = SagaStepStatus::Compensating;

        // Compensate in reverse order
        for step in workflow.steps.iter_mut().rev() {
            if step.status == SagaStepStatus::Completed {
                match self.execute_compensation(&step.compensation).await {
                    Ok(_) => {
                        step.status = SagaStepStatus::Compensated;
                    }
                    Err(e) => {
                        step.error = Some(format!("Compensation failed: {}", e));
                        return Err(e);
                    }
                }
            }
        }

        workflow.status = SagaStepStatus::Compensated;
        Ok(())
    }

    async fn execute_compensation(&self, compensation: &str) -> Result<(), AppError> {
        // Parse and execute compensation logic
        // This is a placeholder for actual compensation execution
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_workflow_state(&self, workflow: &SagaWorkflow) -> Result<(), AppError> {
        let workflow_json = serde_json::to_string(workflow)?;
        
        sqlx::query(
            "INSERT INTO saga_workflows (id, name, state, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET state = $3, status = $4, updated_at = $6",
        )
        .bind(workflow.id)
        .bind(&workflow.name)
        .bind(&workflow_json)
        .bind(format!("{:?}", workflow.status))
        .bind(workflow.created_at)
        .bind(chrono::Utc::now())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn load_workflow_state(&self, workflow_id: Uuid) -> Result<SagaWorkflow, AppError> {
        let row = sqlx::query!(
            "SELECT state FROM saga_workflows WHERE id = $1",
            workflow_id
        )
        .fetch_one(&self.pool)
        .await?;

        let workflow: SagaWorkflow = serde_json::from_str(&row.state)?;
        Ok(workflow)
    }
}
