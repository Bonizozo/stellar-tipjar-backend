use std::sync::Arc;
use std::time::Duration;

use crate::controllers::scheduled_tip_controller as ctrl;
use crate::db::connection::AppState;
use crate::models::scheduled_tip::next_run;

/// Spawns a background loop that processes due scheduled tips every 60 seconds.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = process_due(&state).await {
                tracing::error!(error = %e, "Scheduled tip processor error");
            }
        }
    });
}

async fn process_due(state: &Arc<AppState>) -> anyhow::Result<()> {
    let due = ctrl::due_tips(&state.db).await?;

    for tip in due {
        let id = tip.id;

        // Optimistic lock — skip if another instance grabbed it
        if !ctrl::mark_processing(&state.db, id).await? {
            continue;
        }

        tracing::info!(
            scheduled_tip_id = %id,
            creator = %tip.creator_username,
            amount  = %tip.amount,
            "Processing scheduled tip"
        );

        // Compute next recurrence before mutating state
        let next = tip
            .recurrence_rule
            .as_deref()
            .filter(|_| tip.is_recurring)
            .and_then(|rule| {
                next_run(
                    tip.next_run_at.unwrap_or(tip.scheduled_at),
                    rule,
                    tip.recurrence_end,
                )
            });

        // Enqueue a notification job for the creator
        let notify_result = enqueue_notification(state, &tip.creator_username, id).await;
        if let Err(e) = notify_result {
            tracing::warn!(scheduled_tip_id = %id, error = %e, "Failed to enqueue notification");
        }

        ctrl::mark_completed(&state.db, id, next).await?;

        tracing::info!(
            scheduled_tip_id = %id,
            next_run_at = ?next,
            "Scheduled tip processed"
        );
    }

    Ok(())
}

async fn enqueue_notification(
    state: &Arc<AppState>,
    creator_username: &str,
    scheduled_tip_id: uuid::Uuid,
) -> anyhow::Result<()> {
    // Look up creator to get their ID for the notification payload
    let creator = sqlx::query!(
        "SELECT id FROM creators WHERE username = $1",
        creator_username
    )
    .fetch_optional(&state.db)
    .await?;

    let Some(creator) = creator else {
        tracing::warn!(creator = creator_username, "Creator not found for notification");
        return Ok(());
    };

    use crate::jobs::{JobPayload, JobType, NotificationType};

    // Reuse the existing job queue to send the notification
    let payload = JobPayload::SendNotification {
        creator_id: creator.id,
        tip_id: scheduled_tip_id,
        notification_type: NotificationType::TipReceived,
        recipient_email: format!("{}@placeholder.invalid", creator_username),
    };

    // Fire-and-forget: log if enqueue fails but don't abort processing
    let queue = crate::jobs::queue::JobQueueManager::new(Arc::new(state.db.clone()));
    if let Err(e) = queue.enqueue(JobType::SendNotification, payload, 0, 3).await {
        tracing::warn!(error = %e, "Failed to enqueue scheduled tip notification job");
    }

    Ok(())
}
