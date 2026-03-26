use std::sync::Arc;
use tokio::sync::broadcast;

use crate::db::connection::AppState;
use crate::ws::TipEvent;
use super::{aggregators, anomaly_detector};

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
    if let Err(e) = aggregators::update_creator_stats(&state.db, &event.creator_id, event.amount).await {
        tracing::error!(error = %e, "Failed to update creator stats");
    }

    // 2. Check for anomalies against the freshly updated baseline.
    if let Err(e) = anomaly_detector::check_and_log(&state.db, &event.creator_id, event.amount).await {
        tracing::error!(error = %e, "Anomaly detection failed");
    }
}
