use std::sync::Arc;
use std::time::Duration;

use crate::db::connection::AppState;
use crate::queue::VerificationJob;
use crate::validation::amount::xlm_to_stroops_str;

/// A tip that has been stuck in `pending_verification` for longer than the
/// staleness threshold.
#[derive(Debug, sqlx::FromRow)]
struct StuckTip {
    id: uuid::Uuid,
    transaction_hash: String,
    amount: String,
    creator_username: String,
    #[sqlx(default)]
    tipper_source_account: Option<String>,
}

/// How old a `pending_verification` tip must be before the reconciliation job
/// considers it stuck and re-drives it.
const STUCK_AFTER: Duration = Duration::from_secs(5 * 60); // 5 minutes

/// How often the reconciliation job polls for stuck tips.
const POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn a background reconciliation task that periodically re-enqueues
/// `pending_verification` tips that have not been resolved within the
/// staleness window.
///
/// This provides resilience against queue worker crashes, Horizon outages,
/// or process restarts.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        tracing::info!(
            "Tip reconciliation job started (poll interval {:?})",
            POLL_INTERVAL
        );

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            run_once(Arc::clone(&state)).await;
        }
    });
}

/// Run a single reconciliation pass. Returns the number of tips re-enqueued.
pub async fn run_once(state: Arc<AppState>) -> usize {
    let stuck_tips = match fetch_stuck_tips(&state).await {
        Ok(tips) => tips,
        Err(e) => {
            tracing::error!(error = %e, "Reconciliation: failed to fetch stuck tips");
            return 0;
        }
    };

    if stuck_tips.is_empty() {
        tracing::debug!("Reconciliation: no stuck tips found");
        return 0;
    }

    tracing::info!("Reconciliation: found {} stuck tips", stuck_tips.len());

    let mut enqueued = 0usize;

    for tip in &stuck_tips {
        // Parse amount to stroops; skip tips with unparseable amounts
        let amount_stroops = match xlm_to_stroops_str(&tip.amount) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    tip_id = %tip.id,
                    amount = %tip.amount,
                    error = %e,
                    "Reconciliation: cannot parse tip amount; skipping"
                );
                continue;
            }
        };

        // Look up the creator's wallet address
        let destination = match fetch_creator_wallet(&state, &tip.creator_username).await {
            Ok(Some(addr)) => addr,
            Ok(None) => {
                tracing::warn!(
                    tip_id = %tip.id,
                    creator = %tip.creator_username,
                    "Reconciliation: creator not found; skipping"
                );
                continue;
            }
            Err(e) => {
                tracing::error!(
                    tip_id = %tip.id,
                    error = %e,
                    "Reconciliation: failed to fetch creator wallet"
                );
                continue;
            }
        };

        let job = VerificationJob {
            tip_id: tip.id,
            transaction_hash: tip.transaction_hash.clone(),
            amount_stroops,
            destination,
            expected_memo: None,
            source_account: tip.tipper_source_account.clone().unwrap_or_default(),
            attempt: 0,
        };

        match state.queue.enqueue(job).await {
            Ok(()) => {
                tracing::info!(
                    tip_id = %tip.id,
                    "Reconciliation: re-enqueued stuck tip"
                );
                enqueued += 1;
            }
            Err(e) => {
                tracing::error!(
                    tip_id = %tip.id,
                    error = %e,
                    "Reconciliation: failed to enqueue tip"
                );
            }
        }
    }

    tracing::info!(
        "Reconciliation: re-enqueued {}/{} tips",
        enqueued,
        stuck_tips.len()
    );
    enqueued
}

async fn fetch_stuck_tips(state: &AppState) -> Result<Vec<StuckTip>, sqlx::Error> {
    let stuck_threshold = chrono::Utc::now() - chrono::Duration::from_std(STUCK_AFTER).unwrap();

    sqlx::query_as::<_, StuckTip>(
        r#"
        SELECT id, transaction_hash, amount, creator_username, tipper_source_account
        FROM tips
        WHERE status = 'pending_verification'
          AND created_at < $1
        ORDER BY created_at ASC
        LIMIT 100
        "#,
    )
    .bind(stuck_threshold)
    .fetch_all(&state.db)
    .await
}

async fn fetch_creator_wallet(
    state: &AppState,
    username: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT wallet_address FROM creators WHERE username = $1")
        .bind(username)
        .fetch_optional(&state.db)
        .await
}
