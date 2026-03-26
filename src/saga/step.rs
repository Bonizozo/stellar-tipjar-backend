use std::collections::HashMap;
use uuid::Uuid;

/// Shared mutable context passed through all saga steps.
/// Steps read inputs and write outputs into this map.
#[derive(Debug, Clone, Default)]
pub struct SagaContext {
    pub saga_id: Uuid,
    data: HashMap<String, serde_json::Value>,
}

impl SagaContext {
    pub fn new(saga_id: Uuid) -> Self {
        Self { saga_id, data: HashMap::new() }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl serde::Serialize) {
        self.data.insert(key.into(), serde_json::to_value(value).unwrap_or_default());
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.data.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Completed,
    Compensated,
    Failed,
}

impl StepState {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepState::Pending => "pending",
            StepState::Completed => "completed",
            StepState::Compensated => "compensated",
            StepState::Failed => "failed",
        }
    }
}

/// A single step in a saga: a forward action and its compensating action.
pub struct SagaStep {
    pub name: &'static str,
    pub action: Box<dyn SagaAction>,
    pub compensation: Box<dyn CompensationAction>,
}

#[async_trait::async_trait]
pub trait SagaAction: Send + Sync {
    async fn execute(&self, ctx: &mut SagaContext) -> crate::errors::AppResult<()>;
}

#[async_trait::async_trait]
pub trait CompensationAction: Send + Sync {
    async fn compensate(&self, ctx: &SagaContext) -> crate::errors::AppResult<()>;
}

/// No-op compensation for steps that are inherently idempotent or irreversible.
pub struct NoOpCompensation;

#[async_trait::async_trait]
impl CompensationAction for NoOpCompensation {
    async fn compensate(&self, _ctx: &SagaContext) -> crate::errors::AppResult<()> {
        Ok(())
    }
}
