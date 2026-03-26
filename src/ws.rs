use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::db::connection::AppState;

/// Capacity of the broadcast channel; sized to absorb bursts without dropping slow subscribers.
pub const CHANNEL_CAPACITY: usize = 128;

/// A real-time event emitted whenever a tip is successfully recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipEvent {
    /// Username of the creator who received the tip.
    pub creator_id: String,
    /// Stellar account ID (or username) of the sender.
    pub tipper_id: String,
    /// Tip amount in stroops (1 XLM = 10_000_000 stroops).
    pub amount: u64,
    /// Unix timestamp (seconds) when the tip was recorded.
    pub timestamp: i64,
}

/// WebSocket upgrade handler — registered at GET /ws.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.broadcast_tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, rx))
}

/// Drive a single WebSocket connection, forwarding broadcast events as JSON text frames.
async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<TipEvent>) {
    loop {
        match rx.recv().await {
            Ok(event) => {
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::warn!("ws: failed to serialize TipEvent: {e}");
                        continue;
                    }
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    // Client disconnected — not an application error.
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("ws subscriber lagged, skipping frames (missed {n})");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                // Channel shut down (server stopping).
                break;
            }
        }
    }
}

/// Send a `TipEvent` to all connected WebSocket subscribers.
///
/// A send error means there are no active subscribers; that is not an error condition.
pub async fn broadcast_tip(tx: &broadcast::Sender<TipEvent>, event: TipEvent) {
    if let Err(e) = tx.send(event) {
        tracing::debug!("broadcast_tip: no active subscribers ({e})");
    }
}
