use std::sync::Arc;
use tokio::sync::broadcast;

use super::{aggregators, alerting, anomaly_detector, windowing};
use crate::db::connection::AppState;
use crate::ws::TipEvent;

/// Spawns a background task that consumes `TipEvent`s from the broadcast channel
/// and drives the analytics pipeline (aggregation + anomaly detection).
pub fn spawn(state: Arc<AppState>) {
    let rx = state.broadcast_tx.subscribe();
    tokio::spawn(run(state, rx));
}

async fn run(state: Arc<AppState>, mut rx: broadcast::Receiver<TipEvent>) {
    loop {
        match rx.recv().await {
            Ok(event) => process(&state, event).await,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "Analytics pipeline lagged");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn process(state: &AppState, event: TipEvent) {
    // 1. Update per-creator aggregate stats.
    if let Err(e) =
        aggregators::update_creator_stats(&state.db, &event.creator_id, event.amount).await
    {
        tracing::error!(error = %e, "Failed to update creator stats");
    }

    // 2. Check for anomalies against the freshly updated baseline.
    if let Err(e) =
        anomaly_detector::check_and_log(&state.db, &event.creator_id, event.amount).await
    {
        tracing::error!(error = %e, "Anomaly detection failed");
    }

    // 3. Update tumbling window and evaluate alerts.
    let (window_total, window_count) =
        windowing::update_tumbling_window(&state.db, &event.creator_id, event.amount)
            .await
            .unwrap_or((0, 0));
    if let Err(e) =
        alerting::evaluate(&state.db, &event.creator_id, window_total, window_count).await
    {
        tracing::error!(error = %e, "Alert evaluation failed");
    }
}
