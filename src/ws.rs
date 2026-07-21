use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;

use crate::db::connection::AppState;
use crate::errors::AppError;
use crate::metrics::collectors::{
    AUTH_FAILURES_TOTAL, WS_ACTIVE_CONNECTIONS, WS_EVENTS_DROPPED_TOTAL, WS_EVENTS_SENT_TOTAL,
    WS_SUBSCRIPTIONS,
};
use crate::models::auth::Claims;
use crate::services::auth_service;

/// Default capacity of the broadcast channel; sized to absorb bursts without dropping slow
/// subscribers. Overridable via `WS_CHANNEL_CAPACITY`.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 128;

/// Broadcast channel capacity, read once at startup from `WS_CHANNEL_CAPACITY`.
pub fn channel_capacity() -> usize {
    std::env::var("WS_CHANNEL_CAPACITY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_CHANNEL_CAPACITY)
}

/// Liveness/idle tuning, read once at startup and stored on `AppState` so every connection
/// shares the same values (and so tests can inject short durations without mutating
/// process-wide env vars).
#[derive(Debug, Clone, Copy)]
pub struct WsConfig {
    /// How often the server pings each connection to check liveness.
    pub ping_interval: Duration,
    /// Consecutive missed pongs tolerated before a connection is considered dead.
    pub max_missed_pongs: u32,
    /// How long a connection may sit with zero subscriptions before it is closed as idle.
    pub idle_timeout: Duration,
}

impl WsConfig {
    pub fn from_env() -> Self {
        Self {
            ping_interval: std::env::var("WS_PING_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(30)),
            max_missed_pongs: std::env::var("WS_MAX_MISSED_PONGS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            idle_timeout: std::env::var("WS_IDLE_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(60)),
        }
    }
}

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

/// A channel a client can subscribe to. `Public` is the global tip firehose (opt-in, no
/// auth required); `Creator` scopes delivery to a single creator's tips and requires auth.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Channel {
    Public,
    Creator(String),
}

impl Channel {
    /// Parses the wire form (`"public"` or `"creator:<username>"`). Returns `None` for any
    /// other string, including `"creator:"` with an empty username.
    pub fn parse(s: &str) -> Option<Self> {
        if s == "public" {
            Some(Channel::Public)
        } else if let Some(name) = s.strip_prefix("creator:") {
            if name.is_empty() {
                None
            } else {
                Some(Channel::Creator(name.to_string()))
            }
        } else {
            None
        }
    }

    pub fn as_wire(&self) -> String {
        match self {
            Channel::Public => "public".to_string(),
            Channel::Creator(name) => format!("creator:{name}"),
        }
    }

    /// Whether a valid, authenticated JWT is required to subscribe to this channel.
    pub fn requires_auth(&self) -> bool {
        !matches!(self, Channel::Public)
    }

    /// Whether `event` should be delivered to a connection subscribed to this channel.
    pub fn matches(&self, event: &TipEvent) -> bool {
        match self {
            Channel::Public => true,
            Channel::Creator(name) => name == &event.creator_id,
        }
    }
}

/// Whether any of `subs` wants to receive `event`.
fn should_forward(subs: &HashSet<Channel>, event: &TipEvent) -> bool {
    subs.iter().any(|c| c.matches(event))
}

/// A client-initiated control frame.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ClientOp {
    Subscribe { channel: String },
    Unsubscribe { channel: String },
}

/// Outcome of applying a `ClientOp`, carrying the frame (if any) to send back to the client.
#[derive(Debug, PartialEq, Eq)]
enum OpOutcome {
    Subscribed(Channel),
    Unsubscribed(Channel),
    AuthRequired,
    InvalidChannel,
    InvalidFrame,
}

impl OpOutcome {
    fn into_frame(self) -> String {
        match self {
            OpOutcome::Subscribed(c) => {
                serde_json::json!({"op": "subscribed", "channel": c.as_wire()}).to_string()
            }
            OpOutcome::Unsubscribed(c) => {
                serde_json::json!({"op": "unsubscribed", "channel": c.as_wire()}).to_string()
            }
            OpOutcome::AuthRequired => {
                error_frame("authentication required for this channel")
            }
            OpOutcome::InvalidChannel => error_frame("invalid channel"),
            OpOutcome::InvalidFrame => error_frame("invalid frame"),
        }
    }
}

fn error_frame(message: &str) -> String {
    serde_json::json!({"op": "error", "message": message}).to_string()
}

/// Deterministic lag-policy frame sent when a slow consumer is dropped by the broadcast
/// channel (`broadcast::error::RecvError::Lagged`).
fn gap_frame(missed: u64) -> String {
    serde_json::json!({"op": "gap", "missed": missed}).to_string()
}

/// Per-connection subscription bookkeeping, kept separate from socket I/O so the
/// subscribe/unsubscribe/auth-gating logic can be unit tested without a live socket.
#[derive(Debug, Default)]
struct SubscriptionState {
    authenticated: bool,
    channels: HashSet<Channel>,
}

impl SubscriptionState {
    fn new(authenticated: bool) -> Self {
        Self {
            authenticated,
            channels: HashSet::new(),
        }
    }

    /// Applies a raw client text frame, mutating `self.channels` on success.
    fn apply_text(&mut self, text: &str) -> OpOutcome {
        match serde_json::from_str::<ClientOp>(text) {
            Ok(ClientOp::Subscribe { channel }) => match Channel::parse(&channel) {
                Some(c) if c.requires_auth() && !self.authenticated => OpOutcome::AuthRequired,
                Some(c) => {
                    self.channels.insert(c.clone());
                    OpOutcome::Subscribed(c)
                }
                None => OpOutcome::InvalidChannel,
            },
            Ok(ClientOp::Unsubscribe { channel }) => match Channel::parse(&channel) {
                Some(c) => {
                    self.channels.remove(&c);
                    OpOutcome::Unsubscribed(c)
                }
                None => OpOutcome::InvalidChannel,
            },
            Err(_) => OpOutcome::InvalidFrame,
        }
    }
}

/// Query params accepted on the WS upgrade: `?token=<jwt>&channel=<initial channel>`.
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    token: Option<String>,
    channel: Option<String>,
}

/// Extracts a bearer token from the `Sec-WebSocket-Protocol` header, as browsers cannot set
/// arbitrary headers during the WS handshake. Convention: `bearer.<jwt>`.
fn token_from_protocol_header(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)?;
    let value = value.to_str().ok()?;
    value
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("bearer."))
        .map(str::to_owned)
}

/// WebSocket upgrade handler — registered at GET /ws.
///
/// Authenticates via `?token=` or a `Sec-WebSocket-Protocol: bearer.<jwt>` header. The
/// requested initial `?channel=` (default `public`) is validated before the protocol switch;
/// non-public channels reject unauthenticated upgrades with 401.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let requested = query.channel.as_deref().unwrap_or("public");
    let Some(channel) = Channel::parse(requested) else {
        return AppError::bad_request(format!("invalid channel '{requested}'")).into_response();
    };

    let protocol_token = token_from_protocol_header(&headers);
    let token = query.token.clone().or_else(|| protocol_token.clone());
    let claims = token
        .as_deref()
        .and_then(|t| auth_service::validate_token(t, "access").ok());

    if channel.requires_auth() && claims.is_none() {
        AUTH_FAILURES_TOTAL.with_label_values(&["ws"]).inc();
        return AppError::unauthorized("authentication required for this channel")
            .into_response();
    }

    // Echo the negotiated subprotocol back so browser WebSocket clients (which cannot set
    // custom headers) that authenticated via `Sec-WebSocket-Protocol: bearer.<jwt>` complete
    // the handshake per spec.
    let ws = match protocol_token {
        Some(t) => ws.protocols([format!("bearer.{t}")]),
        None => ws,
    };

    let rx = state.broadcast_tx.subscribe();
    let shutdown_rx = state.ws_shutdown_tx.subscribe();
    let config = state.ws_config;
    ws.on_upgrade(move |socket| handle_socket(socket, rx, shutdown_rx, channel, claims, config))
}

/// Drive a single WebSocket connection: forwards matching broadcast `TipEvent`s, applies the
/// lag policy, answers subscribe/unsubscribe ops, maintains liveness via ping/pong, enforces
/// an idle timeout, and closes gracefully when the server signals shutdown.
async fn handle_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<TipEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
    initial_channel: Channel,
    claims: Option<Claims>,
    config: WsConfig,
) {
    WS_ACTIVE_CONNECTIONS.inc();
    let mut subs = SubscriptionState::new(claims.is_some());
    subs.channels.insert(initial_channel);
    WS_SUBSCRIPTIONS.add(subs.channels.len() as f64);

    let max_missed = config.max_missed_pongs;
    let idle_after = config.idle_timeout;

    let mut ping_ticker = tokio::time::interval(config.ping_interval);
    ping_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping_ticker.tick().await; // first tick fires immediately; consume it.
    let mut missed_pongs: u32 = 0;

    let mut idle_deadline = Instant::now() + idle_after;

    'outer: loop {
        tokio::select! {
            biased;

            _ = tokio::time::sleep_until(idle_deadline), if subs.channels.is_empty() => {
                tracing::debug!("ws: closing idle connection with no subscriptions");
                break 'outer;
            }

            _ = shutdown_rx.changed() => {
                let _ = socket
                    .send(Message::Close(Some(CloseFrame {
                        code: 1001, // going away
                        reason: "server shutting down".into(),
                    })))
                    .await;
                break 'outer;
            }

            _ = ping_ticker.tick() => {
                if missed_pongs >= max_missed {
                    tracing::debug!(missed_pongs, "ws: closing connection after missed pongs");
                    let _ = socket
                        .send(Message::Close(Some(CloseFrame {
                            code: 1000,
                            reason: "ping timeout".into(),
                        })))
                        .await;
                    break 'outer;
                }
                missed_pongs += 1;
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    break 'outer;
                }
            }

            event = rx.recv() => {
                match event {
                    Ok(event) => {
                        if !should_forward(&subs.channels, &event) {
                            continue;
                        }
                        let json = match serde_json::to_string(&event) {
                            Ok(j) => j,
                            Err(e) => {
                                tracing::warn!("ws: failed to serialize TipEvent: {e}");
                                continue;
                            }
                        };
                        if socket.send(Message::Text(json)).await.is_err() {
                            break 'outer;
                        }
                        WS_EVENTS_SENT_TOTAL.inc();
                    }
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(missed, "ws subscriber lagged, sending gap frame");
                        WS_EVENTS_DROPPED_TOTAL.inc_by(missed as f64);
                        if socket.send(Message::Text(gap_frame(missed))).await.is_err() {
                            break 'outer;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break 'outer;
                    }
                }
            }

            incoming = socket.recv() => {
                let before = subs.channels.len();
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let outcome = subs.apply_text(&text);
                        if let OpOutcome::AuthRequired = outcome {
                            AUTH_FAILURES_TOTAL.with_label_values(&["ws"]).inc();
                        }
                        if socket.send(Message::Text(outcome.into_frame())).await.is_err() {
                            break 'outer;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break 'outer;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {
                        missed_pongs = 0;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break 'outer;
                    }
                    Some(Ok(Message::Binary(_))) => {
                        // Binary control frames are not part of the protocol; ignore.
                    }
                    Some(Err(e)) => {
                        tracing::debug!("ws: recv error, closing: {e}");
                        break 'outer;
                    }
                }
                let after = subs.channels.len();
                if after != before {
                    WS_SUBSCRIPTIONS.add(after as f64 - before as f64);
                }
                if after == 0 {
                    idle_deadline = Instant::now() + idle_after;
                }
            }
        }
    }

    WS_SUBSCRIPTIONS.sub(subs.channels.len() as f64);
    WS_ACTIVE_CONNECTIONS.dec();
}

/// Send a `TipEvent` to all connected WebSocket subscribers.
///
/// A send error means there are no active subscribers; that is not an error condition.
pub async fn broadcast_tip(tx: &broadcast::Sender<TipEvent>, event: TipEvent) {
    if let Err(e) = tx.send(event) {
        tracing::debug!("broadcast_tip: no active subscribers ({e})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_for(creator: &str) -> TipEvent {
        TipEvent {
            creator_id: creator.to_string(),
            tipper_id: "tipper".to_string(),
            amount: 1_000_000,
            timestamp: 0,
        }
    }

    #[test]
    fn channel_parses_public_and_creator() {
        assert_eq!(Channel::parse("public"), Some(Channel::Public));
        assert_eq!(
            Channel::parse("creator:alice"),
            Some(Channel::Creator("alice".to_string()))
        );
    }

    #[test]
    fn channel_rejects_malformed_input() {
        assert_eq!(Channel::parse("creator:"), None);
        assert_eq!(Channel::parse("bogus"), None);
        assert_eq!(Channel::parse(""), None);
    }

    #[test]
    fn only_creator_channel_requires_auth() {
        assert!(!Channel::Public.requires_auth());
        assert!(Channel::Creator("alice".to_string()).requires_auth());
    }

    #[test]
    fn public_channel_matches_every_event() {
        assert!(Channel::Public.matches(&event_for("alice")));
        assert!(Channel::Public.matches(&event_for("bob")));
    }

    #[test]
    fn creator_channel_matches_only_its_own_creator() {
        let ch = Channel::Creator("alice".to_string());
        assert!(ch.matches(&event_for("alice")));
        assert!(!ch.matches(&event_for("bob")));
    }

    /// The core cross-creator leakage guarantee: a client subscribed only to creator A never
    /// sees creator B's tips.
    #[test]
    fn subscription_filtering_has_no_cross_creator_leakage() {
        let mut subs = HashSet::new();
        subs.insert(Channel::Creator("alice".to_string()));

        assert!(should_forward(&subs, &event_for("alice")));
        assert!(!should_forward(&subs, &event_for("bob")));
    }

    #[test]
    fn empty_subscriptions_forward_nothing() {
        let subs: HashSet<Channel> = HashSet::new();
        assert!(!should_forward(&subs, &event_for("alice")));
    }

    #[test]
    fn subscribing_to_public_never_requires_auth() {
        let mut subs = SubscriptionState::new(false);
        let outcome = subs.apply_text(r#"{"op":"subscribe","channel":"public"}"#);
        assert_eq!(outcome, OpOutcome::Subscribed(Channel::Public));
        assert!(subs.channels.contains(&Channel::Public));
    }

    #[test]
    fn subscribing_to_creator_channel_unauthenticated_is_rejected() {
        let mut subs = SubscriptionState::new(false);
        let outcome = subs.apply_text(r#"{"op":"subscribe","channel":"creator:alice"}"#);
        assert_eq!(outcome, OpOutcome::AuthRequired);
        assert!(subs.channels.is_empty());
    }

    #[test]
    fn subscribing_to_creator_channel_authenticated_succeeds() {
        let mut subs = SubscriptionState::new(true);
        let outcome = subs.apply_text(r#"{"op":"subscribe","channel":"creator:alice"}"#);
        assert_eq!(
            outcome,
            OpOutcome::Subscribed(Channel::Creator("alice".to_string()))
        );
        assert!(subs.channels.contains(&Channel::Creator("alice".to_string())));
    }

    #[test]
    fn unsubscribe_removes_channel() {
        let mut subs = SubscriptionState::new(true);
        subs.apply_text(r#"{"op":"subscribe","channel":"creator:alice"}"#);
        let outcome = subs.apply_text(r#"{"op":"unsubscribe","channel":"creator:alice"}"#);
        assert_eq!(
            outcome,
            OpOutcome::Unsubscribed(Channel::Creator("alice".to_string()))
        );
        assert!(subs.channels.is_empty());
    }

    #[test]
    fn malformed_channel_string_is_rejected() {
        let mut subs = SubscriptionState::new(true);
        let outcome = subs.apply_text(r#"{"op":"subscribe","channel":"not-a-channel"}"#);
        assert_eq!(outcome, OpOutcome::InvalidChannel);
    }

    #[test]
    fn malformed_json_is_rejected() {
        let mut subs = SubscriptionState::new(true);
        let outcome = subs.apply_text("not json");
        assert_eq!(outcome, OpOutcome::InvalidFrame);
    }

    #[test]
    fn gap_frame_reports_missed_count() {
        let frame = gap_frame(7);
        let parsed: serde_json::Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["op"], "gap");
        assert_eq!(parsed["missed"], 7);
    }

    #[test]
    fn token_from_protocol_header_parses_bearer_prefix() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::SEC_WEBSOCKET_PROTOCOL,
            "bearer.abc123".parse().unwrap(),
        );
        assert_eq!(
            token_from_protocol_header(&headers),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn token_from_protocol_header_absent_returns_none() {
        let headers = HeaderMap::new();
        assert_eq!(token_from_protocol_header(&headers), None);
    }
}

/// End-to-end tests driving the real upgrade handler + connection loop over a live (loopback)
/// WebSocket, exercising behaviour that the pure unit tests above can't reach: HTTP status on
/// rejected upgrades, cross-creator subscription isolation over the wire, the lag-gap frame
/// under a genuinely slow consumer, ping-timeout disconnects, and shutdown drain.
#[cfg(test)]
mod integration {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use axum_test::TestServer;
    use sqlx::postgres::PgPoolOptions;

    fn set_jwt_secret() {
        std::env::set_var("JWT_SECRET", "ws-integration-test-secret");
    }

    fn creator_token(username: &str) -> String {
        set_jwt_secret();
        crate::services::auth_service::generate_tokens(username, "creator")
            .unwrap()
            .access_token
    }

    /// Builds an `AppState` whose non-WS services are inert: the DB pool is lazy (never
    /// dials out, since none of the WS code paths touch the database) and Redis/cache/replica
    /// integrations are disabled. `capacity` sizes the broadcast channel directly so lag tests
    /// don't need to mutate process-wide env vars.
    fn test_state(capacity: usize, config: WsConfig) -> Arc<AppState> {
        set_jwt_secret();
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://user:pass@127.0.0.1/does-not-exist")
            .expect("connect_lazy never dials the network");
        let (broadcast_tx, _) = broadcast::channel(capacity);
        let (ws_shutdown_tx, _) = watch::channel(false);
        Arc::new(AppState {
            db: db.clone(),
            stellar: crate::services::stellar_service::StellarService::new(
                "https://example.invalid".to_string(),
                "testnet".to_string(),
            ),
            performance: Arc::new(crate::db::performance::PerformanceMonitor::new()),
            redis: None,
            broadcast_tx,
            moderation: Arc::new(crate::moderation::ModerationService::new(db.clone())),
            db_circuit_breaker: Arc::new(crate::services::circuit_breaker::CircuitBreaker::new(
                5,
                Duration::from_secs(60),
            )),
            cache: None,
            invalidator: None,
            encryption: Arc::new(crate::crypto::encryption::EncryptionKeyManager::new()),
            replicas: None,
            lock_service: None,
            ws_shutdown_tx,
            ws_config: config,
        })
    }

    fn default_config() -> WsConfig {
        WsConfig {
            ping_interval: Duration::from_secs(30),
            max_missed_pongs: 2,
            idle_timeout: Duration::from_secs(60),
        }
    }

    fn test_server(state: Arc<AppState>) -> TestServer {
        let app = Router::new()
            .route("/ws", get(ws_handler))
            .with_state(state);
        TestServer::builder().http_transport().build(app).unwrap()
    }

    fn event_for(creator: &str) -> TipEvent {
        TipEvent {
            creator_id: creator.to_string(),
            tipper_id: "tipper".to_string(),
            amount: 1_000_000,
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn unauthenticated_upgrade_to_creator_channel_is_rejected() {
        let state = test_state(8, default_config());
        let server = test_server(state);

        let response = server
            .get_websocket("/ws")
            .add_query_param("channel", "creator:alice")
            .await;

        response.assert_status_unauthorized();
    }

    #[tokio::test]
    async fn unauthenticated_upgrade_to_public_channel_succeeds() {
        let state = test_state(8, default_config());
        let server = test_server(state);

        let response = server.get_websocket("/ws").await;

        response.assert_status_switching_protocols();
    }

    #[tokio::test]
    async fn authenticated_upgrade_to_creator_channel_succeeds() {
        let state = test_state(8, default_config());
        let server = test_server(state);
        let token = creator_token("alice");

        let response = server
            .get_websocket("/ws")
            .add_query_param("token", &token)
            .add_query_param("channel", "creator:alice")
            .await;

        response.assert_status_switching_protocols();
    }

    #[tokio::test]
    async fn creator_a_subscriber_never_sees_creator_bs_tips() {
        let state = test_state(8, default_config());
        let server = test_server(Arc::clone(&state));
        let token = creator_token("alice");

        let mut ws = server
            .get_websocket("/ws")
            .add_query_param("token", &token)
            .add_query_param("channel", "creator:alice")
            .await
            .into_websocket()
            .await;

        broadcast_tip(&state.broadcast_tx, event_for("bob")).await;
        broadcast_tip(&state.broadcast_tx, event_for("alice")).await;

        let received: TipEvent = ws.receive_json().await;
        assert_eq!(received.creator_id, "alice");
    }

    #[tokio::test]
    async fn slow_consumer_receives_deterministic_gap_frame() {
        // Tiny capacity + a burst of sends with no intervening `.await` yield point starves
        // the connection task of a chance to drain, guaranteeing a real `Lagged` error.
        let state = test_state(2, default_config());
        let server = test_server(Arc::clone(&state));

        let mut ws = server.get_websocket("/ws").await.into_websocket().await;

        for i in 0..6u64 {
            broadcast_tip(&state.broadcast_tx, event_for(&format!("creator-{i}"))).await;
        }

        let frame: serde_json::Value = ws.receive_json().await;
        assert_eq!(frame["op"], "gap");
        assert!(frame["missed"].as_u64().unwrap() > 0);
    }

    #[tokio::test(start_paused = true)]
    async fn ping_timeout_disconnects_after_missed_pongs() {
        let config = WsConfig {
            ping_interval: Duration::from_millis(10),
            max_missed_pongs: 2,
            idle_timeout: Duration::from_secs(3600),
        };
        let state = test_state(8, config);
        let server = test_server(state);

        let mut ws = server.get_websocket("/ws").await.into_websocket().await;

        // Never send a Pong back. After `max_missed_pongs` unanswered pings the server
        // closes the connection.
        tokio::time::advance(Duration::from_millis(1_000)).await;

        loop {
            match ws.receive_message().await {
                axum_test::WsMessage::Ping(_) => continue,
                axum_test::WsMessage::Close(frame) => {
                    let frame = frame.expect("close frame present");
                    assert_eq!(u16::from(frame.code), 1000);
                    break;
                }
                other => panic!("unexpected frame while waiting for close: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn shutdown_signal_sends_going_away_close_frame() {
        let state = test_state(8, default_config());
        let server = test_server(Arc::clone(&state));

        let mut ws = server.get_websocket("/ws").await.into_websocket().await;

        state.ws_shutdown_tx.send(true).unwrap();

        let frame = loop {
            match ws.receive_message().await {
                axum_test::WsMessage::Close(frame) => break frame.expect("close frame present"),
                other => panic!("unexpected frame while waiting for close: {other:?}"),
            }
        };
        assert_eq!(u16::from(frame.code), 1001);
    }

    #[tokio::test]
    async fn subscribe_and_unsubscribe_round_trip_over_the_wire() {
        let state = test_state(8, default_config());
        let server = test_server(Arc::clone(&state));
        let token = creator_token("alice");

        let mut ws = server
            .get_websocket("/ws")
            .add_query_param("token", &token)
            .await
            .into_websocket()
            .await;

        ws.send_json(&serde_json::json!({"op": "subscribe", "channel": "creator:alice"}))
            .await;
        let ack: serde_json::Value = ws.receive_json().await;
        assert_eq!(ack["op"], "subscribed");
        assert_eq!(ack["channel"], "creator:alice");

        ws.send_json(&serde_json::json!({"op": "unsubscribe", "channel": "creator:alice"}))
            .await;
        let ack: serde_json::Value = ws.receive_json().await;
        assert_eq!(ack["op"], "unsubscribed");
    }

    #[tokio::test]
    async fn unauthenticated_subscribe_to_creator_channel_is_rejected_post_upgrade() {
        let state = test_state(8, default_config());
        let server = test_server(Arc::clone(&state));

        // Public upgrade succeeds without auth...
        let mut ws = server.get_websocket("/ws").await.into_websocket().await;

        // ...but trying to subscribe to a creator channel afterward is still rejected.
        ws.send_json(&serde_json::json!({"op": "subscribe", "channel": "creator:alice"}))
            .await;
        let ack: serde_json::Value = ws.receive_json().await;
        assert_eq!(ack["op"], "error");
    }
}
