use super::workflow::{SagaStepStatus, SagaWorkflow};
use crate::errors::AppError;
use sqlx::PgPool;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

type CompensationFuture = Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>>;
type CompensationFn = Arc<dyn Fn() -> CompensationFuture + Send + Sync>;

pub struct CompensationHandler {
    pool: PgPool,
    hooks: Arc<RwLock<HashMap<String, CompensationFn>>>,
}

impl CompensationHandler {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            hooks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_hook<F, Fut>(&self, name: impl Into<String>, hook: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), AppError>> + Send + 'static,
    {
        self.hooks
            .write()
            .await
            .insert(name.into(), Arc::new(move || Box::pin(hook())));
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
        if let Some(hook) = self.hooks.read().await.get(compensation).cloned() {
            return hook().await;
        }

        // Default to no-op SQL heartbeat so workflow remains recoverable without
        // explicit compensation hooks registered.
        sqlx::query("SELECT 1").execute(&self.pool).await?;
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
