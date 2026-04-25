use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SagaStepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Compensating,
    Compensated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SagaStep {
    pub id: String,
    pub status: SagaStepStatus,
    pub action: String,
    pub compensation: String,
    pub retry_count: i32,
    pub max_retries: i32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SagaWorkflow {
    pub id: Uuid,
    pub name: String,
    pub steps: Vec<SagaStep>,
    pub status: SagaStepStatus,
    pub context: HashMap<String, String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl SagaWorkflow {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            steps: Vec::new(),
            status: SagaStepStatus::Pending,
            context: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    pub fn add_step(
        &mut self,
        id: String,
        action: String,
        compensation: String,
        max_retries: i32,
    ) {
        self.steps.push(SagaStep {
            id,
            status: SagaStepStatus::Pending,
            action,
            compensation,
            retry_count: 0,
            max_retries,
            error: None,
        });
    }

    pub fn get_next_pending_step(&self) -> Option<&SagaStep> {
        self.steps
            .iter()
            .find(|s| s.status == SagaStepStatus::Pending)
    }

    pub fn mark_step_completed(&mut self, step_id: &str) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.status = SagaStepStatus::Completed;
        }
    }

    pub fn mark_step_failed(&mut self, step_id: &str, error: String) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.id == step_id) {
            step.status = SagaStepStatus::Failed;
            step.error = Some(error);
        }
    }

    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|s| s.status == SagaStepStatus::Completed)
    }

    pub fn has_failures(&self) -> bool {
        self.steps.iter().any(|s| s.status == SagaStepStatus::Failed)
    }
}
