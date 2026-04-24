use std::sync::Arc;
use std::time::Duration;

use crate::controllers::subscription_controller as ctrl;
use crate::db::connection::AppState;

/// Spawns a background loop that processes due subscription renewals every 5 minutes.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            if let Err(e) = process_renewals(&state).await {
                tracing::error!(error = %e, "Subscription renewal processor error");
            }
        }
    });
}

async fn process_renewals(state: &Arc<AppState>) -> anyhow::Result<()> {
    let due = ctrl::due_renewals(&state.db).await?;

    for sub in due {
        tracing::info!(
            subscription_id = %sub.id,
            creator = %sub.creator_username,
            subscriber = %sub.subscriber_ref,
            "Processing subscription renewal"
        );

        // Attempt renewal without a transaction hash — payment is recorded as pending.
        // A real implementation would trigger an on-chain payment request here.
        match ctrl::renew_subscription(&state.db, sub.id, None).await {
            Ok(renewed) => {
                tracing::info!(
                    subscription_id = %renewed.id,
                    new_period_end = %renewed.current_period_end,
                    "Subscription renewed"
                );
                dispatch_notification(state, &sub.creator_username, sub.id, "subscription_renewed").await;
            }
            Err(e) => {
                tracing::warn!(
                    subscription_id = %sub.id,
                    error = %e,
                    "Subscription renewal failed, marking past_due"
                );
                let _ = ctrl::mark_past_due(&state.db, sub.id).await;
                dispatch_notification(state, &sub.creator_username, sub.id, "subscription_past_due").await;
            }
        }
    }

    Ok(())
}

async fn dispatch_notification(
    state: &Arc<AppState>,
    creator_username: &str,
    subscription_id: uuid::Uuid,
    event_type: &str,
) {
    let creator = sqlx::query!(
        "SELECT id FROM creators WHERE username = $1",
        creator_username
    )
    .fetch_optional(&state.db)
    .await;

    let creator_id = match creator {
        Ok(Some(row)) => row.id,
        _ => {
            tracing::warn!(creator = creator_username, "Creator not found for subscription notification");
            return;
        }
    };

    use crate::jobs::{JobPayload, JobType, NotificationType};

    let notification_type = if event_type == "subscription_renewed" {
        NotificationType::TipReceived // reuse closest variant; extend NotificationType for production
    } else {
        NotificationType::TipFailed
    };

    let payload = JobPayload::SendNotification {
        creator_id,
        tip_id: subscription_id,
        notification_type,
        recipient_email: format!("{}@placeholder.invalid", creator_username),
    };

    let queue = crate::jobs::queue::JobQueueManager::new(Arc::new(state.db.clone()));
    if let Err(e) = queue.enqueue(JobType::SendNotification, payload, 0, 3).await {
        tracing::warn!(error = %e, "Failed to enqueue subscription notification");
    }
}
