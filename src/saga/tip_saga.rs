use std::sync::Arc;
use uuid::Uuid;

use super::orchestrator::SagaOrchestrator;
use super::step::{CompensationAction, NoOpCompensation, SagaAction, SagaContext, SagaStep};
use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::models::tip::RecordTipRequest;

/// Context keys used between steps.
const KEY_TIP_ID: &str = "tip_id";

// ── Step 1: record the tip in the database (as pending_verification) ──────────

struct RecordTipStep {
    state: Arc<AppState>,
    req: RecordTipRequest,
}

#[async_trait::async_trait]
impl SagaAction for RecordTipStep {
    async fn execute(&self, ctx: &mut SagaContext) -> AppResult<()> {
        let tip = crate::controllers::tip_controller::record_tip(
            &self.state,
            RecordTipRequest {
                username: self.req.username.clone(),
                amount: self.req.amount.clone(),
                tipper_wallet: self.req.tipper_wallet.clone(),
                transaction_hash: self.req.transaction_hash.clone(),
                tipper_source_account: self.req.tipper_source_account.clone(),
                memo: self.req.memo.clone(),
                message: self.req.message.clone(),
                message_visibility: self.req.message_visibility.clone(),
            },
        )
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

// ── Step 2: fire webhook notification (only fires for confirmed tips) ─────────

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
            "status": "pending_verification",
        });
        // Note: "tip.submitted" event – does NOT trigger leaderboard/stats.
        // "tip.confirmed" is emitted by confirm_tip() after on-chain verification.
        crate::webhooks::trigger_webhooks(self.state.db.clone(), "tip.submitted", payload).await;
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
            name: "record_tip",
            action: Box::new(RecordTipStep {
                state: Arc::clone(&state),
                req: RecordTipRequest {
                    username: req.username.clone(),
                    amount: req.amount.clone(),
                    tipper_wallet: req.tipper_wallet.clone(),
                    transaction_hash: req.transaction_hash.clone(),
                    tipper_source_account: req.tipper_source_account.clone(),
                    memo: req.memo.clone(),
                    message: req.message.clone(),
                    message_visibility: req.message_visibility.clone(),
                },
            }),
            compensation: Box::new(DeleteTipCompensation {
                pool: state.db.clone(),
            }),
            max_retries: 1,
            retry_backoff_ms: 200,
        },
        SagaStep {
            name: "notify",
            action: Box::new(NotifyStep {
                state: Arc::clone(&state),
                username: req.username.clone(),
                amount: req.amount.clone(),
            }),
            compensation: Box::new(NoOpCompensation),
            max_retries: 0,
            retry_backoff_ms: 0,
        },
    ];

    orchestrator.execute(saga_id, "tip_processing", steps).await
}
