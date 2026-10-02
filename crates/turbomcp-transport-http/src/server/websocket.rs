//! WebSocket, served as a route on the Streamable HTTP endpoint.
//!
//! WebSocket is not an MCP-spec transport; this is for deployments that
//! already speak it. It rides the HTTP stack rather than running a server of
//! its own, so the endpoint's guards apply before the upgrade exactly as they
//! do to a `POST`: the Origin and Host policy, bearer authentication (a real
//! `401` with its `WWW-Authenticate` challenge and the RFC 9728 metadata, not
//! a close frame after the `101`), both rate limiters, and graceful shutdown.
//! The separate WebSocket server this replaces had none of the accept-loop
//! resilience, no connection cap, no handshake timeout, and returned on
//! shutdown before its connections drained.
//!
//! Each connection is one session: the server's per-connection service tracks
//! its `initialize` handshake and ends the session when the socket closes. The
//! `mcp` subprotocol is selected when the client asks for it, as the official
//! SDKs' clients do.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use turbomcp_core::Identity;
use turbomcp_service::{ServeConfig, ServerHandle};

use super::{
    HttpConfig, HttpState, PeerIp, check_host, check_origin, enforce_auth, enforce_rate_limit,
};
use crate::ws_link::{Frame, Keepalive, Link, Read, WsError, close};

/// The subprotocol the official SDKs negotiate.
const SUBPROTOCOL: &str = "mcp";

/// WebSocket on the HTTP endpoint: where it listens and how it keeps
/// connections alive. Origin policy, authentication, rate limits and the
/// message size limit come from the [`HttpConfig`] it is part of, so the two
/// can't drift apart.
#[derive(Clone)]
pub struct WebSocketConfig {
    path: String,
    ping_interval: Option<Duration>,
    max_idle_pings: Option<u32>,
    max_connections: u32,
    slots: Arc<Semaphore>,
}

impl core::fmt::Debug for WebSocketConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WebSocketConfig")
            .field("path", &self.path)
            .field("ping_interval", &self.ping_interval)
            .field("max_idle_pings", &self.max_idle_pings)
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

impl WebSocketConfig {
    /// Accept WebSocket upgrades on `path` (e.g. `/ws`), pinging after 30s
    /// without inbound traffic, closing after 2 unanswered pings, and holding
    /// at most 1024 connections.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ping_interval: Some(Duration::from_secs(30)),
            max_idle_pings: Some(2),
            max_connections: 1024,
            slots: Arc::new(Semaphore::new(1024)),
        }
    }

    /// Ping a connection that has sent nothing for `interval`, so idle NAT
    /// and proxy timers don't cut it silently (`None` never pings, and so
    /// never notices a peer that went away without closing).
    #[must_use]
    pub fn ping_interval(mut self, interval: Option<Duration>) -> Self {
        self.ping_interval = interval;
        self
    }

    /// Close a connection after `max` consecutive unanswered pings (`None`
    /// keeps pinging a silent peer forever).
    #[must_use]
    pub fn max_idle_pings(mut self, max: Option<u32>) -> Self {
        self.max_idle_pings = max;
        self
    }

    /// Hold at most `max` connections; an upgrade past it answers `503`.
    #[must_use]
    pub fn max_connections(mut self, max: u32) -> Self {
        self.max_connections = max.max(1);
        self.slots = Arc::new(Semaphore::new(self.max_connections as usize));
        self
    }

    fn keepalive(&self) -> Option<Keepalive> {
        self.ping_interval.map(|interval| Keepalive {
            interval,
            max_idle_pings: self.max_idle_pings,
        })
    }

    /// Wait (up to `budget`) for every connection to finish draining.
    pub(super) async fn drained(&self, budget: Duration) {
        let _ = tokio::time::timeout(budget, self.slots.acquire_many(self.max_connections)).await;
    }
}

impl Frame for Message {
    fn text(frame: Bytes) -> Result<Self, WsError> {
        Utf8Bytes::try_from(frame)
            .map(Message::Text)
            .map_err(|_| WsError::Utf8)
    }

    fn ping() -> Self {
        Message::Ping(Bytes::new())
    }

    fn close(code: u16, reason: &'static str) -> Self {
        Message::Close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from_static(reason),
        }))
    }

    fn read(&self) -> Read<'_> {
        match self {
            Message::Text(text) => Read::Data(text.as_bytes()),
            Message::Binary(bytes) => Read::Data(bytes),
            Message::Close(_) => Read::Close,
            Message::Ping(_) | Message::Pong(_) => Read::Skip,
        }
    }
}

#[derive(Clone)]
struct WsState<H: ServerHandle> {
    server: H,
    http: HttpState<H::Service>,
    config: WebSocketConfig,
    max_message_bytes: usize,
    drain: Duration,
}

/// The upgrade route, to merge into the endpoint's router.
pub(super) fn routes<H: ServerHandle>(
    server: H,
    http: HttpState<H::Service>,
    config: &HttpConfig,
    websocket: &WebSocketConfig,
) -> Router {
    Router::new()
        .route(&websocket.path, get(upgrade::<H>))
        .with_state(WsState {
            server,
            http,
            config: websocket.clone(),
            max_message_bytes: config.max_body_bytes,
            drain: config.shutdown_timeout,
        })
}

async fn upgrade<H: ServerHandle>(
    State(state): State<WsState<H>>,
    peer: PeerIp,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Some(rejection) = check_origin(&state.http.origins, &headers) {
        return rejection;
    }
    if let Some(rejection) = check_host(&state.http.hosts, &headers) {
        return rejection;
    }
    let authenticated = match enforce_auth(&state.http, &headers).await {
        Ok(authenticated) => authenticated,
        Err(challenge) => return *challenge,
    };
    let subject = authenticated.as_ref().and_then(|a| a.subject.as_deref());
    let client_ip = peer.client_ip(&state.http.trusted_proxies);
    if let Some(rejection) = enforce_rate_limit(&state.http, subject, client_ip) {
        return rejection;
    }
    let Ok(slot) = Arc::clone(&state.config.slots).try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "websocket connection limit reached",
        )
            .into_response();
    };
    let identity = authenticated.map(|a| a.identity);
    let network = peer.network(
        &state.http.trusted_proxies,
        turbomcp_service::NetworkFacts::websocket(),
    );
    let max = state.max_message_bytes;
    upgrade
        .protocols([SUBPROTOCOL])
        .max_message_size(max)
        .max_frame_size(max)
        .on_upgrade(move |socket| connection(state, socket, identity, network, slot))
}

/// Serve one upgraded socket until either end closes it, the server shuts
/// down, or the credential it authenticated with expires.
async fn connection<H: ServerHandle>(
    state: WsState<H>,
    socket: WebSocket,
    identity: Option<Identity>,
    network: turbomcp_service::NetworkFacts,
    _slot: OwnedSemaphorePermit,
) {
    let shutdown = state.http.shutdown.child_token();
    let link = Link::new(socket, state.config.keepalive())
        .going_away_on(state.http.shutdown.clone())
        .with_network(network);
    let close_with = link.close_with();
    let expiry = identity.as_ref().and_then(expires_at);
    let serving = turbomcp_service::serve_with(
        link,
        state.server.connection(),
        ServeConfig {
            shutdown: shutdown.clone(),
            drain_timeout: state.drain,
            identity,
            ..ServeConfig::default()
        },
    );
    tokio::pin!(serving);
    let result = match expiry {
        None => serving.await,
        Some(deadline) => tokio::select! {
            result = &mut serving => result,
            () = tokio::time::sleep_until(deadline) => {
                // A token that has expired authorizes nothing more; HTTP
                // re-checks it on every request, and so, in effect, does this.
                close_with.set(close::POLICY, "credential expired");
                shutdown.cancel();
                serving.await
            }
        },
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "websocket connection ended with an error");
    }
}

/// When a bearer token's `exp` claim (seconds since the epoch) passes.
fn expires_at(identity: &Identity) -> Option<tokio::time::Instant> {
    let exp = identity.claim("exp")?.as_u64()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(tokio::time::Instant::now() + Duration::from_secs(exp.saturating_sub(now)))
}
