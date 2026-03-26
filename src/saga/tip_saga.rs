use std::sync::Arc;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::models::tip::RecordTipRequest;
use super::orchestrator::SagaOrchestrator;
use super::step::{CompensationAction, NoOpCompensation, SagaAction, SagaContext, SagaStep};

/// Context keys used between steps.
const KEY_TIP_ID: &str = "tip_id";
const KEY_TX_HASH: &str = "transaction_hash";

// ── Step 1: verify the Stellar transaction ────────────────────────────────────

struct VerifyTransactionStep {
    state: Arc<AppState>,
    tx_hash: String,
}

#[async_trait::async_trait]
impl SagaAction for VerifyTransactionStep {
    async fn execute(&self, ctx: &mut SagaContext) -> AppResult<()> {
        self.state.stellar.verify_transaction(&self.tx_hash).await?;
        ctx.set(KEY_TX_HASH, &self.tx_hash);
        Ok(())
    }
}

// ── Step 2: record the tip in the database ────────────────────────────────────

struct RecordTipStep {
    state: Arc<AppState>,
    req: RecordTipRequest,
}

#[async_trait::async_trait]
impl SagaAction for RecordTipStep {
    async fn execute(&self, ctx: &mut SagaContext) -> AppResult<()> {
        let tip = crate::controllers::tip_controller::record_tip(&self.state, RecordTipRequest {
            username: self.req.username.clone(),
            amount: self.req.amount.clone(),
            transaction_hash: self.req.transaction_hash.clone(),
        })
        .await?;
        ctx.set(KEY_TIP_ID, tip.id);
        Ok(())
    }
}

struct DeleteTipCompensation {
    pool: sqlx::PgPool,
}

#[async_trait::async_trait]
impl CompensationAction for DeleteTipCompensation {
    async fn compensate(&self, ctx: &SagaContext) -> AppResult<()> {
        if let Some(tip_id) = ctx.get::<Uuid>(KEY_TIP_ID) {
            sqlx::query("DELETE FROM tips WHERE id = $1")
                .bind(tip_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
}

// ── Step 3: fire webhook notification ─────────────────────────────────────────

struct NotifyStep {
    state: Arc<AppState>,
    username: String,
    amount: String,
}

#[async_trait::async_trait]
impl SagaAction for NotifyStep {
    async fn execute(&self, ctx: &mut SagaContext) -> AppResult<()> {
        let tip_id: Option<Uuid> = ctx.get(KEY_TIP_ID);
        let payload = serde_json::json!({
            "tip_id": tip_id,
            "creator_username": self.username,
            "amount": self.amount,
        });
        crate::webhooks::trigger_webhooks(self.state.db.clone(), "tip.recorded", payload).await;
        Ok(())
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Execute the full tip-processing saga.
/// Returns the saga context (contains `tip_id`) on success.
pub async fn run_tip_saga(state: Arc<AppState>, req: RecordTipRequest) -> AppResult<SagaContext> {
    let saga_id = Uuid::new_v4();
    let orchestrator = SagaOrchestrator::new(state.db.clone());

    let steps: Vec<SagaStep> = vec![
        SagaStep {
            name: "verify_transaction",
            action: Box::new(VerifyTransactionStep {
                state: Arc::clone(&state),
                tx_hash: req.transaction_hash.clone(),
            }),
            compensation: Box::new(NoOpCompensation),
        },
        SagaStep {
            name: "record_tip",
            action: Box::new(RecordTipStep {
                state: Arc::clone(&state),
                req: RecordTipRequest {
                    username: req.username.clone(),
                    amount: req.amount.clone(),
                    transaction_hash: req.transaction_hash.clone(),
                },
            }),
            compensation: Box::new(DeleteTipCompensation { pool: state.db.clone() }),
        },
        SagaStep {
            name: "notify",
            action: Box::new(NotifyStep {
                state: Arc::clone(&state),
                username: req.username.clone(),
                amount: req.amount.clone(),
            }),
            compensation: Box::new(NoOpCompensation),
        },
    ];

    orchestrator.execute(saga_id, "tip_processing", steps).await
}
