use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use super::step::{CompensationAction, SagaAction, SagaContext, SagaStep, StepState};

pub struct SagaOrchestrator {
    pub pool: PgPool,
}

impl SagaOrchestrator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Execute a saga: run each step in order, compensating in reverse on failure.
    pub async fn execute(
        &self,
        saga_id: Uuid,
        saga_type: &str,
        steps: Vec<SagaStep>,
    ) -> AppResult<SagaContext> {
        let mut ctx = SagaContext::new(saga_id);

        self.persist_saga(saga_id, saga_type).await?;

        for (i, step) in steps.iter().enumerate() {
            match step.action.execute(&mut ctx).await {
                Ok(()) => {
                    self.log_step(saga_id, i, step.name, StepState::Completed, None).await?;
                    self.update_saga_step(saga_id, i).await?;
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!(saga_id = %saga_id, step = step.name, error = %msg, "Saga step failed");
                    self.log_step(saga_id, i, step.name, StepState::Failed, Some(&msg)).await?;
                    self.compensate(saga_id, &steps[..i], &ctx).await;
                    self.update_saga_state(saga_id, "compensated").await?;
                    return Err(AppError::internal());
                }
            }
        }

        self.update_saga_state(saga_id, "completed").await?;
        Ok(ctx)
    }

    /// Run compensations in reverse order for all completed steps.
    async fn compensate(&self, saga_id: Uuid, completed: &[SagaStep], ctx: &SagaContext) {
        for (i, step) in completed.iter().enumerate().rev() {
            match step.compensation.compensate(ctx).await {
                Ok(()) => {
                    tracing::info!(saga_id = %saga_id, step = step.name, "Compensation succeeded");
                    let _ = self.log_step(saga_id, i, step.name, StepState::Compensated, None).await;
                }
                Err(e) => {
                    // Compensation failure requires manual intervention — log and continue.
                    tracing::error!(saga_id = %saga_id, step = step.name, error = %e, "Compensation failed — manual intervention required");
                }
            }
        }
    }

    async fn persist_saga(&self, saga_id: Uuid, saga_type: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO saga_instances (id, saga_type, state) VALUES ($1, $2, 'pending')
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(saga_id)
        .bind(saga_type)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_saga_step(&self, saga_id: Uuid, step: usize) -> AppResult<()> {
        sqlx::query(
            "UPDATE saga_instances SET current_step = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(saga_id)
        .bind(step as i32 + 1)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_saga_state(&self, saga_id: Uuid, state: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE saga_instances SET state = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(saga_id)
        .bind(state)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn log_step(
        &self,
        saga_id: Uuid,
        index: usize,
        name: &str,
        state: StepState,
        error: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO saga_step_logs (saga_id, step_index, step_name, state, error)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(saga_id)
        .bind(index as i32)
        .bind(name)
        .bind(state.as_str())
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
