//! # turbomcp-transport-ws
//!
//! A WebSocket transport for MCP: JSON-RPC frames carried one-per-WebSocket-
//! message over a bidirectional [`tokio_tungstenite`] stream. Like stdio (and
//! unlike Streamable HTTP), a WebSocket is a long-lived bidirectional channel,
//! so it plugs straight into the [`serve`](turbomcp_service::serve) driver.
//!
//! WebSocket is *not* an MCP-spec transport (the spec defines stdio + Streamable
//! HTTP); this is a convenience for deployments that already speak WS. Framing
//! (one message = one frame) is this crate's job; turning a frame into a value
//! is the [`Codec`]'s. Control frames (ping/pong) are handled by the library and
//! skipped here; a `Close` frame is a clean end-of-stream.
//!
//! ## Production posture (parity with the HTTP transport)
//!
//! [`WsConfig`] carries the same guards `HttpConfig` applies per request, moved
//! to where a long-lived socket needs them:
//!
//! - **Origin policy** — validated during the upgrade handshake (DNS-rebinding
//!   defense). Default mirrors HTTP: requests *without* an `Origin` header
//!   (non-browser clients) pass; any browser `Origin` is rejected until
//!   allowlisted ([`WsConfig::allow_origin`]) or opened
//!   ([`WsConfig::allow_any_origin`]).
//! - **Bearer auth** — the upgrade request's `Authorization` header is
//!   validated by the same [`HttpAuthenticator`] seam the HTTP transport uses,
//!   right after the handshake (the handshake callback is synchronous; JWKS
//!   validation is not). Rejection closes the socket with WebSocket close code
//!   `1008` (policy violation). The authenticated principal is stamped into
//!   every inbound message for the connection's lifetime
//!   ([`ServeConfig::identity`]) — the dispatcher lifts it into
//!   `RequestContext::identity` exactly as it does for HTTP.
//! - **Message-size limit** — [`WsConfig::max_message_bytes`] (default 1 MiB,
//!   matching HTTP's body limit) is enforced by the WebSocket library.
//! - **Idle keepalive + liveness reaping** — after [`WsConfig::ping_interval`]
//!   (default 30s) without an inbound frame, the transport sends a WebSocket
//!   `Ping` so NAT/proxy idle timers don't silently kill the connection. A live
//!   peer's library answers with a `Pong` (an inbound frame), which resets the
//!   idle count; a peer that is open but unreachable answers nothing, and after
//!   [`WsConfig::max_idle_pings`] unanswered probes (default 2 ⇒ ~90s) the
//!   transport ends the stream so the connection's task, buffers, and file
//!   descriptor are reclaimed rather than held indefinitely.
//! - **Graceful shutdown** — the accept loop stops when the
//!   [`ServeConfig::shutdown`] token fires, and each live connection drains
//!   through the serve driver's two-phase drain.
//!
//! TLS: terminate `wss://` at your ingress/proxy (the accept path takes any
//! `AsyncRead + AsyncWrite`, so a TLS acceptor can also be layered in front of
//! [`WebSocketTransport::accept`] directly). The client side supports `wss://`
//! natively ([`connect`]).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use turbomcp_codec::{Codec, CodecError, DefaultCodec};
use turbomcp_core::JsonRpcMessage;
use turbomcp_service::{
    AuthDecision, HttpAuthenticator, McpService, ProtocolError, ServeConfig, Transport,
};

/// Failures from the WebSocket transport.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WsError {
    /// An I/O error on the underlying socket.
    #[error("websocket i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// A WebSocket protocol error.
    #[error("websocket protocol error: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    /// A frame could not be encoded/decoded.
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    /// An outbound frame was not valid UTF-8 (encodings are always JSON text).
    #[error("frame was not valid UTF-8")]
    Utf8,
}

/// Erase into the service-layer boundary error so a binary that serves over
/// more than one transport can `?` every entry point against a single
/// `Result<(), ProtocolError>` — the type `serve_stdio` already returns.
impl From<WsError> for ProtocolError {
    fn from(err: WsError) -> Self {
        match err {
            WsError::Io(e) => ProtocolError::Transport(format!("websocket: {e}")),
            WsError::Ws(e) => ProtocolError::Transport(format!("websocket: {e}")),
            WsError::Codec(e) => ProtocolError::Parse(e.to_string()),
            WsError::Utf8 => ProtocolError::Internal("websocket frame was not valid UTF-8".into()),
        }
    }
}

/// Server-side configuration for [`serve_websocket_with`]: the upgrade-time
/// guards (Origin policy, bearer auth), per-connection limits, and the serve
/// driver's [`ServeConfig`].
#[derive(Clone)]
pub struct WsConfig {
    origins: OriginPolicy,
    authenticator: Option<Arc<dyn HttpAuthenticator>>,
    max_message_bytes: usize,
    ping_interval: Option<Duration>,
    max_idle_pings: Option<u32>,
    serve: ServeConfig,
}

#[derive(Clone)]
enum OriginPolicy {
    /// Origin-less requests pass; a present `Origin` must be in the list.
    Allowlist(Vec<String>),
    /// Any Origin (or none) passes. For dev / non-browser deployments.
    Any,
}

impl std::fmt::Debug for WsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsConfig")
            .field("max_message_bytes", &self.max_message_bytes)
            .field("ping_interval", &self.ping_interval)
            .field("max_idle_pings", &self.max_idle_pings)
            .field("authenticator", &self.authenticator.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for WsConfig {
    fn default() -> Self {
        Self {
            origins: OriginPolicy::Allowlist(Vec::new()),
            authenticator: None,
            max_message_bytes: 1 << 20, // 1 MiB, matching HttpConfig
            ping_interval: Some(Duration::from_secs(30)),
            max_idle_pings: Some(2),
            serve: ServeConfig::default(),
        }
    }
}

impl WsConfig {
    /// Default configuration: Origin-less clients only, no auth, 1 MiB
    /// messages, 30s idle pings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allow browser connections from `origin` (e.g. `https://app.example.com`).
    /// Chainable; case-insensitive exact match.
    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        let origin = origin.into().to_ascii_lowercase();
        match &mut self.origins {
            OriginPolicy::Allowlist(list) => list.push(origin),
            OriginPolicy::Any => self.origins = OriginPolicy::Allowlist(vec![origin]),
        }
        self
    }

    /// Accept any `Origin` header (development / non-browser deployments; the
    /// DNS-rebinding guard is off).
    #[must_use]
    pub fn allow_any_origin(mut self) -> Self {
        self.origins = OriginPolicy::Any;
        self
    }

    /// Require a valid bearer token on the upgrade request, validated by
    /// `authenticator` (the same seam `HttpConfig::with_authenticator` uses).
    /// The authenticated principal is stamped into every message on the
    /// connection.
    #[must_use]
    pub fn with_authenticator(mut self, authenticator: Arc<dyn HttpAuthenticator>) -> Self {
        self.authenticator = Some(authenticator);
        self
    }

    /// Cap inbound WebSocket message size (default 1 MiB).
    #[must_use]
    pub fn max_message_bytes(mut self, bytes: usize) -> Self {
        self.max_message_bytes = bytes;
        self
    }

    /// Send a WebSocket `Ping` after this long without an inbound frame
    /// (default 30s); `None` disables idle pings (and, with them, liveness
    /// reaping).
    #[must_use]
    pub fn ping_interval(mut self, interval: Option<Duration>) -> Self {
        self.ping_interval = interval;
        self
    }

    /// End the connection after this many consecutive unanswered idle pings
    /// (default `Some(2)`). A peer that stays open but replies to nothing — not
    /// even the library's automatic `Pong` — is treated as dead once the count
    /// is reached, so its resources are reclaimed. `None` pings forever without
    /// ever reaping (keepalive only). Has no effect when
    /// [`ping_interval`](Self::ping_interval) is `None`.
    #[must_use]
    pub fn max_idle_pings(mut self, max: Option<u32>) -> Self {
        self.max_idle_pings = max;
        self
    }

    /// The serve-driver configuration applied to each connection (shutdown
    /// token, drain deadline, concurrency bound). The shutdown token also
    /// stops the accept loop.
    #[must_use]
    pub fn with_serve_config(mut self, config: ServeConfig) -> Self {
        self.serve = config;
        self
    }

    /// Whether the upgrade request's `Origin` header passes the policy.
    fn origin_allowed(&self, origin: Option<&str>) -> bool {
        match (&self.origins, origin) {
            (OriginPolicy::Any, _) | (OriginPolicy::Allowlist(_), None) => true,
            (OriginPolicy::Allowlist(list), Some(origin)) => {
                list.contains(&origin.to_ascii_lowercase())
            }
        }
    }
}

/// JSON-RPC over a WebSocket: each message is one complete frame.
///
/// Generic over the underlying byte stream `S` (a `TcpStream`, a TLS stream, an
/// in-memory duplex, …) so it is reusable and unit-testable.
pub struct WebSocketTransport<S, C = DefaultCodec> {
    stream: WebSocketStream<S>,
    codec: C,
    ping_interval: Option<Duration>,
    max_idle_pings: Option<u32>,
}

impl<S, C: Codec> WebSocketTransport<S, C> {
    /// Wrap an established [`WebSocketStream`] with the given codec.
    pub fn new(stream: WebSocketStream<S>, codec: C) -> Self {
        Self {
            stream,
            codec,
            ping_interval: None,
            max_idle_pings: None,
        }
    }

    /// Probe an idle connection with a WebSocket `Ping` after `interval`
    /// without inbound traffic (`None`, the default here, never pings).
    #[must_use]
    pub fn with_ping_interval(mut self, interval: Option<Duration>) -> Self {
        self.ping_interval = interval;
        self
    }

    /// Reap the connection after `max` consecutive unanswered idle pings
    /// (`None`, the default here, never reaps). See
    /// [`WsConfig::max_idle_pings`].
    #[must_use]
    pub fn with_max_idle_pings(mut self, max: Option<u32>) -> Self {
        self.max_idle_pings = max;
        self
    }
}

impl<S> WebSocketTransport<S, DefaultCodec>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Complete the server side of the WebSocket handshake over an accepted
    /// socket, then wrap it with the [`DefaultCodec`].
    ///
    /// # Errors
    /// Fails if the handshake does not complete.
    pub async fn accept(socket: S) -> Result<Self, WsError> {
        let stream = tokio_tungstenite::accept_async(socket).await?;
        Ok(Self::new(stream, DefaultCodec::default()))
    }

    /// Adopt a raw byte stream as a WebSocket endpoint in `role` **without** a
    /// handshake — for tests over an in-memory duplex, or when the handshake was
    /// performed elsewhere.
    pub async fn from_raw(socket: S, role: Role) -> Self {
        let stream = WebSocketStream::from_raw_socket(socket, role, None).await;
        Self::new(stream, DefaultCodec::default())
    }

    /// Close the socket with a policy-violation close frame (code `1008`) —
    /// how a server refuses a connection after the upgrade already completed
    /// (e.g. failed bearer auth).
    async fn close_policy(mut self, reason: &str) {
        let frame = CloseFrame {
            code: CloseCode::Policy,
            reason: reason.to_owned().into(),
        };
        let _ = self.stream.close(Some(frame)).await;
    }
}

impl<S, C> Transport for WebSocketTransport<S, C>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Codec,
{
    type Error = WsError;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        let bytes = self.codec.encode(&msg)?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|_| WsError::Utf8)?;
        self.stream.send(Message::text(text)).await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        let mut idle_pings: u32 = 0;
        loop {
            let frame = match self.ping_interval {
                Some(interval) => {
                    match tokio::time::timeout(interval, self.stream.next()).await {
                        Ok(frame) => frame,
                        Err(_idle) => {
                            // Idle past the interval. If we've already probed the
                            // allowed number of times with no inbound frame in
                            // between (not even a Pong), the peer is open but
                            // unreachable — end the stream so its resources are
                            // reclaimed instead of held forever.
                            if self.max_idle_pings.is_some_and(|max| idle_pings >= max) {
                                tracing::debug!(
                                    idle_pings,
                                    "websocket peer unresponsive to pings; closing"
                                );
                                return Ok(None);
                            }
                            idle_pings += 1;
                            // Probe liveness: a live peer's library answers with
                            // a Pong (an inbound frame, resetting the count).
                            self.stream.send(Message::Ping(Vec::new().into())).await?;
                            continue;
                        }
                    }
                }
                None => self.stream.next().await,
            };
            // Any inbound frame — including the automatic Pong — proves the peer
            // is alive this interval.
            idle_pings = 0;
            let Some(frame) = frame else {
                return Ok(None);
            };
            match frame? {
                Message::Text(t) => return Ok(Some(self.codec.decode(t.as_bytes())?)),
                Message::Binary(b) => return Ok(Some(self.codec.decode(&b)?)),
                Message::Close(_) => return Ok(None),
                // Ping/Pong are answered by the library; ignore and read on.
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            }
        }
    }

    async fn close(mut self) -> Result<(), Self::Error> {
        self.stream.close(None).await?;
        Ok(())
    }
}

/// Connect to a WebSocket MCP server at `url` (`ws://…` or `wss://…`) and return
/// a ready [`WebSocketTransport`].
///
/// # Errors
/// Fails if the connection or handshake does not complete.
pub async fn connect(
    url: &str,
) -> Result<WebSocketTransport<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, WsError> {
    let (stream, _resp) = tokio_tungstenite::connect_async(url).await?;
    Ok(WebSocketTransport::new(stream, DefaultCodec::default()))
}

/// Serve WebSocket connections on `listener` with the default [`WsConfig`],
/// building one service per connection from `make_service`.
///
/// The per-connection factory matters for the legacy (`2025-11-25`) path: a
/// `LegacySessionAdapter` holds one connection's session, so each connection
/// needs its own — e.g.
/// `serve_websocket(listener, move || LegacySessionAdapter::new(dispatcher.clone()))`.
/// A bare (draft-only) dispatcher can simply be cloned:
/// `serve_websocket(listener, move || dispatcher.clone())`.
///
/// # Errors
/// Returns only if accepting a connection fails; per-connection errors are
/// logged and do not stop the accept loop.
pub async fn serve_websocket<S, F>(listener: TcpListener, make_service: F) -> Result<(), WsError>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: McpService + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    serve_websocket_with(listener, make_service, WsConfig::default()).await
}

/// [`serve_websocket`] with explicit [`WsConfig`] (Origin policy, bearer auth,
/// limits, keepalive, serve driver settings).
///
/// Returns cleanly when [`ServeConfig::shutdown`] (inside
/// [`WsConfig::with_serve_config`]) fires: the accept loop stops and each live
/// connection drains on its own task.
///
/// # Errors
/// Returns only if accepting a connection fails.
pub async fn serve_websocket_with<S, F>(
    listener: TcpListener,
    make_service: F,
    config: WsConfig,
) -> Result<(), WsError>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: McpService + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    let shutdown = config.serve.shutdown.clone();
    loop {
        let accepted = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => accepted,
        };
        let (socket, peer) = accepted?;
        let service = make_service();
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_connection(socket, service, &config).await {
                tracing::debug!(%peer, error = %e, "websocket connection ended with error");
            }
        });
    }
}

/// Handshake (with Origin policy + size limits), authenticate, then drive the
/// serve loop for one accepted socket.
// The handshake callback's Result<Response, ErrorResponse> signature is
// dictated by tungstenite's Callback trait; its Err size is not ours to shrink.
#[allow(clippy::result_large_err)]
async fn serve_connection<IO, S>(
    socket: IO,
    service: S,
    config: &WsConfig,
) -> Result<(), Box<dyn std::error::Error>>
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S: McpService + Clone,
    S::Future: Send + 'static,
{
    // The handshake callback is synchronous: enforce the Origin policy there
    // (pure check) and capture the Authorization header for the async bearer
    // validation right after.
    let mut authorization: Option<String> = None;
    let callback = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let origin = req
            .headers()
            .get("origin")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if !config.origin_allowed(origin.as_deref()) {
            let reject = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(403)
                .body(Some("origin not allowed".to_owned()))
                .expect("static response");
            return Err(reject);
        }
        authorization = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok(resp)
    };
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(config.max_message_bytes))
        .max_frame_size(Some(config.max_message_bytes));
    let stream =
        tokio_tungstenite::accept_hdr_async_with_config(socket, callback, Some(ws_config)).await?;
    let transport = WebSocketTransport::new(stream, DefaultCodec::default())
        .with_ping_interval(config.ping_interval)
        .with_max_idle_pings(config.max_idle_pings);

    // Bearer auth (async: JWKS-backed validators may fetch keys). There is no
    // post-upgrade 401 in WebSocket, so a rejection is close code 1008.
    let mut identity = None;
    if let Some(authenticator) = &config.authenticator {
        match authenticator.authenticate(authorization.as_deref()).await {
            AuthDecision::Allow(principal) => identity = Some(principal),
            AuthDecision::Challenge { status, .. } => {
                tracing::debug!(status, "websocket bearer auth rejected");
                transport.close_policy("authentication failed").await;
                return Ok(());
            }
        }
    }

    let serve_config = ServeConfig {
        identity,
        ..config.serve.clone()
    };
    turbomcp_service::serve_with(transport, service, serve_config).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::{JsonRpcRequest, JsonRpcResponse, RequestId};

    #[test]
    fn ws_error_bridges_into_protocol_error() {
        let io = WsError::Io(std::io::Error::other("accept failed"));
        assert!(matches!(
            ProtocolError::from(io),
            ProtocolError::Transport(m) if m.contains("accept failed")
        ));
        assert!(matches!(
            ProtocolError::from(WsError::Utf8),
            ProtocolError::Internal(_)
        ));
    }

    #[tokio::test]
    async fn round_trips_frames_over_a_duplex() {
        // Two endpoints joined by an in-memory duplex, no TCP or handshake.
        let (a, b) = tokio::io::duplex(4096);
        let mut client = WebSocketTransport::from_raw(a, Role::Client).await;
        let mut server = WebSocketTransport::from_raw(b, Role::Server).await;

        // client → server request
        let req = JsonRpcRequest::new(1, "ping", None);
        client.send(JsonRpcMessage::Request(req)).await.unwrap();
        let got = server.recv().await.unwrap().expect("a frame");
        let JsonRpcMessage::Request(r) = got else {
            panic!("expected a request")
        };
        assert_eq!(r.method, "ping");

        // server → client response
        server
            .send(JsonRpcMessage::Response(JsonRpcResponse::success(
                RequestId::from(1),
                serde_json::json!({ "ok": true }),
            )))
            .await
            .unwrap();
        let got = client.recv().await.unwrap().expect("a frame");
        let JsonRpcMessage::Response(r) = got else {
            panic!("expected a response")
        };
        assert_eq!(r.result.unwrap()["ok"], true);
    }

    #[tokio::test]
    async fn close_frame_is_clean_eof() {
        let (a, b) = tokio::io::duplex(1024);
        let client = WebSocketTransport::from_raw(a, Role::Client).await;
        let mut server = WebSocketTransport::from_raw(b, Role::Server).await;
        client.close().await.unwrap();
        assert!(server.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn idle_ping_keeps_recv_alive() {
        let (a, b) = tokio::io::duplex(1024);
        let client = WebSocketTransport::from_raw(a, Role::Client).await;
        let mut server = WebSocketTransport::from_raw(b, Role::Server)
            .await
            .with_ping_interval(Some(Duration::from_millis(20)));

        // The server sits idle past several ping intervals, then the client
        // finally sends; recv must have survived the idle stretch (pinging,
        // not erroring or returning EOF).
        let (client, got) = tokio::join!(
            async move {
                tokio::time::sleep(Duration::from_millis(90)).await;
                let mut client = client;
                client
                    .send(JsonRpcMessage::Request(JsonRpcRequest::new(
                        7, "late", None,
                    )))
                    .await
                    .unwrap();
                client
            },
            server.recv()
        );
        let got = got.unwrap().expect("late frame");
        let JsonRpcMessage::Request(r) = got else {
            panic!("expected a request")
        };
        assert_eq!(r.method, "late");
        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn unresponsive_peer_is_reaped_after_max_idle_pings() {
        let (a, b) = tokio::io::duplex(1024);
        // The client endpoint exists but is never polled or written to, so it
        // never sends a Pong — the "open but unreachable" case.
        let _client = WebSocketTransport::from_raw(a, Role::Client).await;
        let mut server = WebSocketTransport::from_raw(b, Role::Server)
            .await
            .with_ping_interval(Some(Duration::from_millis(10)))
            .with_max_idle_pings(Some(2));

        // recv must probe, get no pong, and end the stream — never hang.
        let reaped = tokio::time::timeout(Duration::from_secs(2), server.recv())
            .await
            .expect("recv returns rather than hanging on a dead peer");
        assert!(
            reaped.unwrap().is_none(),
            "an unresponsive peer surfaces as clean EOF"
        );
    }

    #[test]
    fn origin_policy_defaults_reject_browser_origins() {
        let cfg = WsConfig::default();
        assert!(cfg.origin_allowed(None), "origin-less clients pass");
        assert!(!cfg.origin_allowed(Some("https://evil.example")));

        let cfg = WsConfig::default().allow_origin("https://App.Example.com");
        assert!(cfg.origin_allowed(Some("https://app.example.com")));
        assert!(cfg.origin_allowed(Some("HTTPS://APP.EXAMPLE.COM")));
        assert!(!cfg.origin_allowed(Some("https://other.example")));

        let cfg = WsConfig::default().allow_any_origin();
        assert!(cfg.origin_allowed(Some("https://anything.example")));
    }
}
