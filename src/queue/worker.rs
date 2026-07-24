use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::db::connection::AppState;
use crate::services::stellar_service::{TipVerifyRequest, VerifyOutcome};
use super::VerificationJob;

/// Maximum attempts before a job is abandoned (tip left as pending for reconciliation).
const MAX_ATTEMPTS: u32 = 5;

/// Spawn a background task that drains the verification job queue and
/// drives `pending_verification` tips to `confirmed` or `rejected`.
pub fn spawn_worker(
    state: Arc<AppState>,
    mut receiver: mpsc::Receiver<VerificationJob>,
) {
    tokio::spawn(async move {
        tracing::info!("Verification queue worker started");

        while let Some(job) = receiver.recv().await {
            let state_clone = Arc::clone(&state);
            tokio::spawn(async move {
                process_job(state_clone, job).await;
            });
        }

        tracing::warn!("Verification queue channel closed – worker exiting");
    });
}

async fn process_job(state: Arc<AppState>, job: VerificationJob) {
    let tip_id = job.tip_id;
    let attempt = job.attempt;

    tracing::info!(
        tip_id = %tip_id,
        tx_hash = %job.transaction_hash,
        attempt = attempt,
        "Processing verification job"
    );

    if attempt >= MAX_ATTEMPTS {
        tracing::error!(
            tip_id = %tip_id,
            "Max verification attempts reached – leaving tip in pending_verification for reconciliation"
        );
        return;
    }

    let verify_req = TipVerifyRequest {
        transaction_hash: job.transaction_hash.clone(),
        amount_stroops: job.amount_stroops,
        destination: job.destination.clone(),
        expected_memo: job.expected_memo.clone(),
        source_account: job.source_account.clone(),
    };

    match state.verifier.verify_tip(&verify_req).await {
        Ok(VerifyOutcome::Confirmed) => {
            tracing::info!(tip_id = %tip_id, "Tip confirmed by on-chain verification");
            if let Err(e) = crate::controllers::tip_controller::confirm_tip(&state, tip_id).await {
                tracing::error!(tip_id = %tip_id, error = %e, "Failed to confirm tip in database");
            }
        }
        Ok(VerifyOutcome::Rejected { reason }) => {
            tracing::warn!(tip_id = %tip_id, reason = %reason, "Tip rejected by on-chain verification");
            if let Err(e) = crate::controllers::tip_controller::reject_tip(&state, tip_id, &reason).await {
                tracing::error!(tip_id = %tip_id, error = %e, "Failed to reject tip in database");
            }
        }
        Err(e) => {
            // Transient error (network, circuit breaker) – re-enqueue with backoff
            tracing::warn!(
                tip_id = %tip_id,
                error = %e,
                attempt = attempt,
                "Verification failed transiently; re-enqueueing"
            );

            let backoff = Duration::from_secs(2u64.saturating_pow(attempt + 1).min(64));
            tokio::time::sleep(backoff).await;

            let retry_job = VerificationJob {
                attempt: attempt + 1,
                ..job
            };

            if let Err(send_err) = state.queue.enqueue(retry_job).await {
                tracing::error!(
                    tip_id = %tip_id,
                    error = %send_err,
                    "Failed to re-enqueue job; reconciliation will retry"
                );
            }
        }
    }
}
