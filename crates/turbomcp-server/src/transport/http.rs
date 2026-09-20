//! HTTP transport implementation.
//!
//! Provides MCP 2025-11-25 Streamable HTTP transport with:
//! - POST for JSON-RPC requests
//! - GET for SSE (Server-Sent Events) for server push
//!
//! # Protocol Compliance
//!
//! This implementation follows the MCP 2025-11-25 streamable HTTP shape:
//! - POST `/` or `/mcp` - JSON-RPC request/response
//! - GET `/` or `/mcp` - optional Server-Sent Events stream
//! - DELETE `/` or `/mcp` - explicit session termination
//! - `Mcp-Session-Id` header for session correlation
//!
//! # Streams
//!
//! A POST answers with a single `application/json` object unless the handler
//! emits something for the client mid-request — sampling, elicitation,
//! progress — in which case it upgrades to `text/event-stream` and carries that
//! traffic plus the final response, per §Sending Messages item 6. The
//! standalone GET stream carries only messages unrelated to a running request,
//! which is what §Listening for Messages item 4 reserves it for.
//!
//! Both kinds of stream are resumable: event IDs are `{session}-{stream}-{seq}`,
//! each stream keeps a bounded history, and a GET carrying `Last-Event-ID`
//! re-attaches to the named stream and replays what it missed.
//!
//! # Version-Aware Routing
//!
//! Per-session version-aware routing is active. After a successful `initialize`
//! handshake, the negotiated [`ProtocolVersion`] is stored in [`SessionManager`]
//! keyed by `Mcp-Session-Id`. All subsequent requests for that session are
//! dispatched through [`router::route_request_versioned`], ensuring correct
//! adapter filtering and method availability for the negotiated spec version.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tower_http::limit::RequestBodyLimitLayer;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::jsonrpc::{JsonRpcResponse as CoreJsonRpcResponse, JsonRpcResponsePayload};
use turbomcp_transport::security::{
    OriginConfig, SecurityHeaders, extract_client_ip, extract_client_ip_with_trust, validate_origin,
};
use turbomcp_types::{ClientCapabilities, ProtocolVersion};
use uuid::Uuid;

use crate::config::{RateLimiter, ServerConfig};
use crate::context::{Cancellable, McpSession, RequestContext, SessionFuture};
use crate::router::{self, JsonRpcIncoming, JsonRpcOutgoing};

use super::{PendingHandlerGuard, jsonrpc_id_key};

/// Maximum HTTP request body size for MCP requests.
///
/// This is intentionally larger than the core `MAX_MESSAGE_SIZE` (1MB) because
/// HTTP transport may need to handle larger payloads (e.g., base64-encoded images
/// in tool responses or large resource uploads). Individual message validation
/// still applies the core limit after decompression where applicable.
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// SSE keep-alive interval.
const SSE_KEEP_ALIVE_SECS: u64 = 30;

/// Maximum in-flight server-to-client requests per HTTP session.
const MAX_PENDING_SERVER_REQUESTS: usize = 64;

/// Timeout for server-to-client request responses over Streamable HTTP.
const SERVER_REQUEST_TIMEOUT_SECS: u64 = 60;

/// Maximum events retained per stream for `Last-Event-ID` replay.
const MAX_REPLAY_EVENTS: usize = 64;

/// Maximum SSE streams retained per session, attached or resumable.
///
/// A stream entry outlives its connection so a reconnect can replay from it,
/// so without a cap a client could grow a session without bound by opening
/// and dropping GETs.
const MAX_RETAINED_STREAMS: usize = 8;

type PendingServerResponse = oneshot::Sender<McpResult<serde_json::Value>>;
type PendingServerRequests = Arc<Mutex<HashMap<String, PendingServerResponse>>>;

/// One SSE event: its `{session}-{stream}-{seq}` id and its payload.
///
/// Payloads are `Arc<str>` so routing and broadcast share one allocation
/// instead of copying the full message per send.
type SseEvent = (String, Arc<str>);

/// One SSE stream belonging to a session.
#[derive(Debug)]
struct StreamState {
    /// Sender for the attached connection, `None` once it drops.
    ///
    /// The entry outlives the connection on purpose: §Resumability lets a
    /// client reconnect with `Last-Event-ID` and pick the stream back up, and
    /// it can only do that if the stream's cursor and history survived.
    sender: Option<mpsc::UnboundedSender<SseEvent>>,
    /// Whether this is a standalone GET stream, and so eligible to carry
    /// messages unrelated to any running request.
    ///
    /// POST streams are addressed explicitly by id and must never be picked by
    /// that heuristic — a GET opened mid-call would otherwise steal the
    /// request's sampling traffic.
    listening: bool,
    /// Cursor for the next event on this stream.
    next_seq: u64,
    /// Events already sent, newest last, bounded by [`MAX_REPLAY_EVENTS`].
    history: VecDeque<(u64, SseEvent)>,
}

impl StreamState {
    fn new(sender: mpsc::UnboundedSender<SseEvent>, listening: bool) -> Self {
        Self {
            sender: Some(sender),
            listening,
            // The primer takes seq 0, so messages start at 1. §Resumability
            // requires event IDs unique within the session, and reusing 0 would
            // make the primer and the first message indistinguishable on replay.
            next_seq: 1,
            history: VecDeque::new(),
        }
    }

    /// Stamp the next event id on `message` and hand it to the connection.
    ///
    /// Only a delivered event is retained. An event the transport never
    /// accepted was reported to its caller as undelivered — `ctx.sample()`
    /// surfaces an error and drops its pending entry — so replaying it later
    /// would resurrect a request the server has already given up on.
    fn emit(&mut self, session_id: &str, stream_id: &str, message: &str) -> bool {
        let Some(sender) = self.sender.as_ref() else {
            return false;
        };

        let seq = self.next_seq;
        let event: SseEvent = (
            format!("{session_id}-{stream_id}-{seq}"),
            Arc::from(message),
        );
        if sender.send(event.clone()).is_err() {
            self.sender = None;
            return false;
        }

        self.next_seq = seq.saturating_add(1);
        while self.history.len() >= MAX_REPLAY_EVENTS {
            self.history.pop_front();
        }
        self.history.push_back((seq, event));
        true
    }

    /// Events this stream sent after `last_seq`, for replay on reconnect.
    fn replay_after(&self, last_seq: u64) -> Vec<SseEvent> {
        self.history
            .iter()
            .filter(|(seq, _)| *seq > last_seq)
            .map(|(_, event)| event.clone())
            .collect()
    }
}

/// Split a `{session}-{stream}-{seq}` event id back into its parts.
///
/// Parsed from the right: the session id is a hyphenated UUID, so only this
/// direction is unambiguous.
fn parse_event_id(event_id: &str) -> Option<(&str, &str, u64)> {
    let (head, seq) = event_id.rsplit_once('-')?;
    let (session_id, stream_id) = head.rsplit_once('-')?;
    Some((session_id, stream_id, seq.parse().ok()?))
}

/// Per-session data tracked by SessionManager.
///
/// The MCP 2025-11-25 spec (§Multiple Connections) says a server "MUST send
/// each of its JSON-RPC messages on only one of the connected streams; that
/// is, it MUST NOT broadcast the same message across multiple streams."
/// We therefore track streams as a list and route each outbound message to
/// exactly one of them.
#[derive(Debug)]
struct SessionData {
    /// Ordered list of this session's SSE streams (newest last).
    streams: Vec<(String, StreamState)>,
    /// Negotiated protocol version (set after successful initialize).
    protocol_version: Option<ProtocolVersion>,
    /// Client capabilities captured from the successful initialize request.
    client_capabilities: Option<ClientCapabilities>,
    /// Pending responses for server-initiated requests sent over SSE.
    pending_server_requests: PendingServerRequests,
    /// Monotonic server request counter. IDs are rendered as `s-{n}`.
    next_server_request_id: u64,
    /// Cancellation tokens for this session's in-flight handlers, keyed by
    /// JSON-RPC request id.
    ///
    /// Per-session, not server-wide. One HTTP server multiplexes many clients,
    /// so a flat map keyed by request id alone would let any client cancel
    /// another client's request by guessing an id.
    pending_handlers: Arc<DashMap<String, CancellationToken>>,
}

/// Session manager for SSE connections.
///
/// Low-level type. Normally `pub(crate)`. Exposed publicly (with `#[doc(hidden)]`)
/// only under the `internal-bench` feature so that benchmarks can drive the
/// real hot paths (including the `Arc<str>` subscriber change).
#[cfg(not(feature = "internal-bench"))]
#[derive(Clone, Debug)]
pub(crate) struct SessionManager {
    /// Map of session ID to per-session data.
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
}

#[cfg(feature = "internal-bench")]
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct SessionManager {
    /// Map of session ID to per-session data.
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    #[allow(dead_code)]
    fn new_inner() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session manager.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) fn new() -> Self {
        Self::new_inner()
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub fn new() -> Self {
        Self::new_inner()
    }

    #[allow(dead_code)]
    async fn create_session_inner(
        &self,
        _initialize_request_id: Option<&serde_json::Value>,
    ) -> String {
        let session_id = Uuid::new_v4().to_string();

        self.sessions.write().await.insert(
            session_id.clone(),
            SessionData {
                streams: Vec::new(),
                protocol_version: None,
                client_capabilities: None,
                pending_server_requests: Arc::new(Mutex::new(HashMap::new())),
                next_server_request_id: 1,
                pending_handlers: Arc::new(DashMap::new()),
            },
        );

        tracing::debug!("Created SSE session: {}", session_id);
        session_id
    }

    /// Create a new session and return the session ID.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) async fn create_session(
        &self,
        initialize_request_id: Option<&serde_json::Value>,
    ) -> String {
        self.create_session_inner(initialize_request_id).await
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub async fn create_session(
        &self,
        initialize_request_id: Option<&serde_json::Value>,
    ) -> String {
        self.create_session_inner(initialize_request_id).await
    }

    /// Remove a session.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) async fn remove_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().await.remove(session_id).is_some();
        if removed {
            tracing::debug!("Removed session: {}", session_id);
        }
        removed
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub async fn remove_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().await.remove(session_id).is_some();
        if removed {
            tracing::debug!("Removed session: {}", session_id);
        }
        removed
    }

    #[allow(dead_code)]
    async fn subscribe_session_inner(
        &self,
        session_id: &str,
    ) -> Option<mpsc::UnboundedReceiver<SseEvent>> {
        self.open_stream(session_id, true)
            .await
            .map(|(_, _, rx)| rx)
    }

    /// Subscribe to an existing session's SSE stream.
    ///
    /// Each subscribe returns a dedicated [`mpsc::UnboundedReceiver`] that
    /// only receives messages routed to this subscriber — never broadcasts.
    #[cfg(not(feature = "internal-bench"))]
    #[allow(dead_code)]
    pub(crate) async fn subscribe_session(
        &self,
        session_id: &str,
    ) -> Option<mpsc::UnboundedReceiver<SseEvent>> {
        self.subscribe_session_inner(session_id).await
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub async fn subscribe_session(
        &self,
        session_id: &str,
    ) -> Option<mpsc::UnboundedReceiver<SseEvent>> {
        self.subscribe_session_inner(session_id).await
    }

    /// Open a new SSE stream on this session.
    ///
    /// Returns the stream id, the id of the primer event the caller should emit
    /// (§Sending Messages item 6), and the receiver. `listening` marks the
    /// standalone GET stream; POST streams are addressed by id only.
    async fn open_stream(
        &self,
        session_id: &str,
        listening: bool,
    ) -> Option<(String, String, mpsc::UnboundedReceiver<SseEvent>)> {
        let mut sessions = self.sessions.write().await;
        let data = sessions.get_mut(session_id)?;

        let stream_id = Uuid::new_v4().simple().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        data.streams
            .push((stream_id.clone(), StreamState::new(tx, listening)));
        Self::evict_excess_streams(data);

        let primer_id = format!("{session_id}-{stream_id}-0");
        Some((stream_id, primer_id, rx))
    }

    /// Re-attach to the stream a `Last-Event-ID` names, replaying what it missed.
    ///
    /// Returns `None` — meaning "open a fresh stream instead" — for a malformed
    /// id, an unknown stream, or an id naming another session. Never replaying
    /// across sessions or streams is what §Resumability requires: "The server
    /// MUST NOT replay messages that would have been delivered on a different
    /// stream."
    async fn resume_stream(
        &self,
        session_id: &str,
        last_event_id: &str,
    ) -> Option<(String, Vec<SseEvent>, mpsc::UnboundedReceiver<SseEvent>)> {
        let (event_session, stream_id, last_seq) = parse_event_id(last_event_id)?;
        if event_session != session_id {
            tracing::warn!(
                session_id,
                last_event_id,
                "Ignoring a Last-Event-ID that names a different session"
            );
            return None;
        }
        let stream_id = stream_id.to_string();

        let mut sessions = self.sessions.write().await;
        let data = sessions.get_mut(session_id)?;
        let (_, state) = data.streams.iter_mut().find(|(id, _)| *id == stream_id)?;

        let (tx, rx) = mpsc::unbounded_channel();
        state.sender = Some(tx);
        Some((stream_id, state.replay_after(last_seq), rx))
    }

    /// Route one payload to a named stream, recording it for replay.
    async fn send_to_stream(&self, session_id: &str, stream_id: &str, message: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        let Some(data) = sessions.get_mut(session_id) else {
            return false;
        };
        let Some((_, state)) = data.streams.iter_mut().find(|(id, _)| id == stream_id) else {
            return false;
        };
        state.emit(session_id, stream_id, message)
    }

    /// Forget a stream outright.
    ///
    /// Used when a POST turned out not to need a stream after all: keeping the
    /// entry would waste one of the session's retained slots on a stream that
    /// never carried an event.
    async fn close_stream(&self, session_id: &str, stream_id: &str) {
        if let Some(data) = self.sessions.write().await.get_mut(session_id) {
            data.streams.retain(|(id, _)| id != stream_id);
        }
    }

    /// Keep the retained-stream list bounded.
    ///
    /// Detached streams go first, since they cost a slot for a replay nobody
    /// has asked for. If every retained stream is still attached, the oldest
    /// goes and its connection ends with it.
    fn evict_excess_streams(data: &mut SessionData) {
        while data.streams.len() > MAX_RETAINED_STREAMS {
            let victim = data
                .streams
                .iter()
                .position(|(_, state)| state.sender.is_none())
                .unwrap_or(0);
            data.streams.remove(victim);
        }
    }

    /// Route one payload to exactly one of a session's listening streams.
    ///
    /// §Multiple Connections: the server "MUST NOT broadcast the same message
    /// across multiple streams". The newest attached listening stream wins,
    /// which gives a fresh GET priority over a stale one without closing
    /// streams that are merely idle.
    fn route_to_listening_stream(session_id: &str, data: &mut SessionData, message: &str) -> bool {
        for index in (0..data.streams.len()).rev() {
            let (stream_id, state) = &mut data.streams[index];
            if !state.listening || state.sender.is_none() {
                continue;
            }
            let stream_id = stream_id.clone();
            if state.emit(session_id, &stream_id, message) {
                return true;
            }
        }
        false
    }

    /// Check whether a session exists.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    #[allow(dead_code)]
    async fn send_to_session_inner(&self, session_id: &str, message: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        let Some(data) = sessions.get_mut(session_id) else {
            return false;
        };
        Self::route_to_listening_stream(session_id, data, message)
    }

    /// Send a message to one listening stream for the given session.
    ///
    /// Per the MCP Multiple Connections rule, this routes the message to
    /// exactly one of the session's currently connected GET streams (the most
    /// recently opened live one). Returns `true` if the message was delivered.
    ///
    /// Exposed under the `internal-bench` feature for `benches/sse_throughput.rs`;
    /// treat as crate-internal otherwise.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) async fn send_to_session(&self, session_id: &str, message: &str) -> bool {
        self.send_to_session_inner(session_id, message).await
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub async fn send_to_session(&self, session_id: &str, message: &str) -> bool {
        self.send_to_session_inner(session_id, message).await
    }

    #[allow(dead_code)]
    async fn broadcast_inner(&self, message: &str) {
        let mut sessions = self.sessions.write().await;
        for (session_id, data) in sessions.iter_mut() {
            if !Self::route_to_listening_stream(session_id, data, message) {
                tracing::warn!("No live subscriber for session {}", session_id);
            }
        }
    }

    /// Broadcast a message to one subscriber per session.
    ///
    /// Iterates every session and routes to a single live subscriber
    /// following the same per-session rule as [`Self::send_to_session`].
    ///
    /// Reserved for server-initiated push (not yet wired). Exposed under the
    /// `internal-bench` feature for `benches/sse_throughput.rs`; treat as
    /// crate-internal otherwise.
    #[cfg(not(feature = "internal-bench"))]
    #[allow(dead_code)]
    pub(crate) async fn broadcast(&self, message: &str) {
        self.broadcast_inner(message).await
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub async fn broadcast(&self, message: &str) {
        self.broadcast_inner(message).await
    }

    /// Get the number of active sessions.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Store the initialized protocol version and client capabilities.
    pub(crate) async fn set_initialized(
        &self,
        session_id: &str,
        version: ProtocolVersion,
        client_capabilities: ClientCapabilities,
    ) {
        if let Some(data) = self.sessions.write().await.get_mut(session_id) {
            data.protocol_version = Some(version);
            data.client_capabilities = Some(client_capabilities);
        }
    }

    /// Retrieve the negotiated protocol version for a session.
    pub(crate) async fn get_protocol_version(&self, session_id: &str) -> Option<ProtocolVersion> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .and_then(|data| data.protocol_version.clone())
    }

    /// Retrieve initialized client capabilities for a session.
    pub(crate) async fn get_client_capabilities(
        &self,
        session_id: &str,
    ) -> Option<ClientCapabilities> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .and_then(|data| data.client_capabilities.clone())
    }

    /// Register a pending server-to-client request and return its JSON-RPC id.
    async fn register_pending_server_request(
        &self,
        session_id: &str,
        response_tx: PendingServerResponse,
    ) -> McpResult<String> {
        let (request_id, pending) = {
            let mut sessions = self.sessions.write().await;
            let Some(data) = sessions.get_mut(session_id) else {
                return Err(McpError::transport("HTTP session not found"));
            };

            let request_id = format!("s-{}", data.next_server_request_id);
            data.next_server_request_id = data.next_server_request_id.saturating_add(1);
            (request_id, Arc::clone(&data.pending_server_requests))
        };

        let mut pending = pending.lock().await;
        if pending.len() >= MAX_PENDING_SERVER_REQUESTS {
            return Err(McpError::server_overloaded());
        }

        pending.insert(request_id.clone(), response_tx);
        Ok(request_id)
    }

    /// Remove a pending server request without completing it.
    async fn remove_pending_server_request(&self, session_id: &str, request_id: &str) -> bool {
        let Some(pending) = self.pending_server_requests(session_id).await else {
            return false;
        };

        pending.lock().await.remove(request_id).is_some()
    }

    /// Complete a pending server request from a client POSTed JSON-RPC response.
    async fn complete_pending_server_response(
        &self,
        session_id: &str,
        response: CoreJsonRpcResponse,
    ) -> Result<(), StatusCode> {
        let Some(request_id) = response.id.as_request_id().map(ToString::to_string) else {
            return Err(StatusCode::BAD_REQUEST);
        };

        let Some(pending) = self.pending_server_requests(session_id).await else {
            return Err(StatusCode::NOT_FOUND);
        };

        let Some(response_tx) = pending.lock().await.remove(&request_id) else {
            tracing::warn!(
                session_id,
                request_id,
                "Received response for unknown HTTP server request"
            );
            return Err(StatusCode::BAD_REQUEST);
        };

        let result = match response.payload {
            JsonRpcResponsePayload::Success { result } => Ok(result),
            JsonRpcResponsePayload::Error { error } => {
                Err(McpError::from_rpc_code(error.code, error.message))
            }
        };

        response_tx
            .send(result)
            .map_err(|_| StatusCode::BAD_REQUEST)
    }

    async fn pending_server_requests(&self, session_id: &str) -> Option<PendingServerRequests> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|data| Arc::clone(&data.pending_server_requests))
    }

    /// Register a cancellation token for an in-flight request on this session.
    ///
    /// Returns the token to install in the request context plus a guard that
    /// removes the registry entry on every exit path (success, error, panic,
    /// future drop), matching the line / channel / websocket transports.
    ///
    /// `None` when the session is gone, in which case the request simply runs
    /// without a token, exactly as a sessionless request does today.
    async fn register_pending_handler(
        &self,
        session_id: &str,
        key: String,
    ) -> Option<(CancellationToken, PendingHandlerGuard)> {
        let handlers = {
            let sessions = self.sessions.read().await;
            Arc::clone(&sessions.get(session_id)?.pending_handlers)
        };

        let token = CancellationToken::new();
        if handlers.insert(key.clone(), token.clone()).is_some() {
            // Sequential id reuse is fine. *Concurrent* reuse is not: the
            // client cannot match two responses carrying one id, and this just
            // overwrote the first handler's token. Report it rather than
            // refusing to serve, matching line.rs.
            tracing::warn!(
                request_id = %key,
                "Request id reused while the first is still in flight"
            );
        }

        let guard = PendingHandlerGuard::new(Arc::clone(&handlers), Some(key));
        Some((token, guard))
    }

    /// Signal the in-flight handler registered under `key` for this session.
    ///
    /// An unknown or already-finished id is a no-op, which is what the
    /// cancellation utility requires of a receiver.
    async fn cancel_pending_handler(&self, session_id: &str, key: &str) -> bool {
        let handlers = {
            let sessions = self.sessions.read().await;
            match sessions.get(session_id) {
                Some(data) => Arc::clone(&data.pending_handlers),
                None => return false,
            }
        };

        match handlers.remove(key) {
            Some((_, token)) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

/// Bidirectional HTTP/SSE session handle used by request handlers.
#[derive(Debug, Clone)]
struct HttpSessionHandle {
    session_id: String,
    session_manager: SessionManager,
    request_timeout: Duration,
    /// Id of the stream opened by the POST that is currently being served.
    request_stream: Option<String>,
}

impl HttpSessionHandle {
    fn new(session_id: impl Into<String>, session_manager: SessionManager) -> Self {
        Self {
            session_id: session_id.into(),
            session_manager,
            request_timeout: Duration::from_secs(SERVER_REQUEST_TIMEOUT_SECS),
            request_stream: None,
        }
    }

    fn with_request_stream(mut self, stream_id: Option<String>) -> Self {
        self.request_stream = stream_id;
        self
    }

    /// Deliver one payload to the client.
    ///
    /// Prefers the SSE stream opened for the request being served: §Sending
    /// Messages item 6 is where request-related traffic belongs, and §Listening
    /// for Messages item 4 reserves the standalone GET stream for messages
    /// *unrelated* to a running request. Falls back to the GET stream when the
    /// POST was answered with plain JSON (a client whose `Accept` omits
    /// `text/event-stream`), or when the request stream is already closed.
    async fn deliver(&self, payload: &str) -> bool {
        if let Some(ref stream_id) = self.request_stream
            && self
                .session_manager
                .send_to_stream(&self.session_id, stream_id, payload)
                .await
        {
            return true;
        }

        self.session_manager
            .send_to_session(&self.session_id, payload)
            .await
    }
}

impl McpSession for HttpSessionHandle {
    fn client_capabilities<'a>(&'a self) -> SessionFuture<'a, Option<ClientCapabilities>> {
        Box::pin(async move {
            Ok(self
                .session_manager
                .get_client_capabilities(&self.session_id)
                .await)
        })
    }

    fn call<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
    ) -> SessionFuture<'a, serde_json::Value> {
        Box::pin(async move {
            let (response_tx, response_rx) = oneshot::channel();
            let request_id = self
                .session_manager
                .register_pending_server_request(&self.session_id, response_tx)
                .await?;

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            });
            let payload = serde_json::to_string(&request)
                .map_err(|e| McpError::serialization(e.to_string()))?;

            if !self.deliver(&payload).await {
                self.session_manager
                    .remove_pending_server_request(&self.session_id, &request_id)
                    .await;
                return Err(McpError::unavailable(
                    "No active SSE stream for HTTP session",
                ));
            }

            match tokio::time::timeout(self.request_timeout, response_rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(McpError::transport("HTTP session response channel closed")),
                Err(_) => {
                    self.session_manager
                        .remove_pending_server_request(&self.session_id, &request_id)
                        .await;
                    Err(McpError::timeout(format!(
                        "Timed out waiting for response to server request {request_id}"
                    )))
                }
            }
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: serde_json::Value) -> SessionFuture<'a, ()> {
        Box::pin(async move {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            });
            let payload = serde_json::to_string(&notification)
                .map_err(|e| McpError::serialization(e.to_string()))?;

            if self.deliver(&payload).await {
                Ok(())
            } else {
                Err(McpError::unavailable(
                    "No active SSE stream for HTTP session",
                ))
            }
        })
    }
}

/// Run a handler on HTTP transport with full MCP Streamable HTTP support.
///
/// This includes:
/// - POST `/` and `/mcp` for JSON-RPC requests
/// - GET `/sse` for Server-Sent Events stream
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to (e.g., "0.0.0.0:8080")
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::transport::http;
///
/// http::run(&handler, "0.0.0.0:8080").await?;
/// ```
pub async fn run<H: McpHandler>(handler: &H, addr: &str) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let app = build_router(handler.clone(), None, None);

    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    tracing::info!(
        "MCP server listening on http://{} (GET/POST/DELETE /, /mcp; GET /sse)",
        socket_addr
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(None))
    .await
    .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// Wait for SIGINT (Ctrl-C) and, on Unix, SIGTERM. Returns when either fires.
///
/// On signal, axum stops accepting new connections and gives in-flight requests up
/// to `drain` to complete. Pre-3.1 the HTTP transport had no shutdown hook at all
/// — SIGTERM aborted in-flight requests mid-response.
async fn shutdown_signal(drain: Option<Duration>) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, draining HTTP server");
    if let Some(drain) = drain {
        // Give the runtime a chance to land the signal before axum starts dropping
        // listeners; the actual drain happens inside axum::serve once this future
        // resolves. We bound the wait so a stuck request can't block exit forever.
        tokio::time::sleep(drain.min(Duration::from_secs(60))).await;
    }
}

/// Run a handler on HTTP transport with custom configuration.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to
/// * `config` - Server configuration (rate limits, etc.)
pub async fn run_with_config<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
) -> McpResult<()> {
    run_with_shutdown(handler, addr, config, None).await
}

/// Variant of [`run_with_config`] that accepts an explicit graceful-shutdown drain
/// timeout. Used by the server builder to thread `with_graceful_shutdown(...)` all
/// the way down to axum.
pub async fn run_with_shutdown<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
    graceful_shutdown: Option<Duration>,
) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let rate_limiter = config
        .rate_limit
        .as_ref()
        .map(|cfg| Arc::new(RateLimiter::new(cfg.clone())));
    let app = build_router(handler.clone(), rate_limiter, Some(config.clone()));

    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    let rate_limit_info = config
        .rate_limit
        .as_ref()
        .map(|cfg| {
            format!(
                " (rate limit: {}/{}s)",
                cfg.max_requests,
                cfg.window.as_secs()
            )
        })
        .unwrap_or_default();

    tracing::info!(
        "MCP server listening on http://{}{} (GET/POST/DELETE /, /mcp; GET /sse)",
        socket_addr,
        rate_limit_info
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(graceful_shutdown))
    .await
    .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// HTTP state with SSE support and optional rate limiting.
#[derive(Clone)]
pub(crate) struct SseState<H: McpHandler> {
    handler: H,
    session_manager: SessionManager,
    rate_limiter: Option<Arc<RateLimiter>>,
    config: Option<ServerConfig>,
}

pub(crate) fn build_router<H: McpHandler>(
    handler: H,
    rate_limiter: Option<Arc<RateLimiter>>,
    config: Option<ServerConfig>,
) -> Router {
    let max_body_size = config
        .as_ref()
        .map_or(MAX_BODY_SIZE, |config| config.max_message_size);
    let state = SseState {
        handler,
        session_manager: SessionManager::new(),
        rate_limiter,
        config,
    };

    Router::new()
        .route(
            "/",
            post(handle_json_rpc::<H>)
                .get(handle_sse::<H>)
                .delete(handle_delete_session::<H>),
        )
        .route(
            "/mcp",
            post(handle_json_rpc::<H>)
                .get(handle_sse::<H>)
                .delete(handle_delete_session::<H>),
        )
        .route("/sse", get(handle_sse::<H>))
        // DefaultBodyLimit sets the extractor hint for Json<T>/Bytes, while
        // RequestBodyLimitLayer enforces the cap at the middleware layer so
        // oversized bodies are rejected with 413 Payload Too Large before
        // our handler (which takes Request<Body>) ever reads the stream.
        .layer(DefaultBodyLimit::max(max_body_size))
        .layer(RequestBodyLimitLayer::new(max_body_size))
        .with_state(state)
}

/// Route a request with per-session version tracking.
///
/// On `initialize`:
/// - Routes through `route_request_with_config` for protocol negotiation.
/// - On success, extracts the negotiated `protocolVersion` from the response
///   and stores it in the session manager for subsequent requests.
///
/// On all other methods when the session has a stored version:
/// - Routes through `route_request_versioned` for adapter-filtered dispatch.
///
/// On all other cases (pre-init or no session):
/// - Routes through `route_request_with_config` which handles validation.
async fn route_with_version_tracking<H: McpHandler>(
    handler: &H,
    request: router::JsonRpcIncoming,
    session_manager: &SessionManager,
    config: Option<&ServerConfig>,
    session_id: Option<&str>,
    request_stream: Option<String>,
) -> router::JsonRpcOutgoing {
    // Held for the life of the dispatch: dropping it de-registers the
    // cancellation token so a late `notifications/cancelled` matches nothing.
    let (ctx, _pending_guard) = http_request_context(
        session_manager,
        session_id,
        request.id.as_ref(),
        request_stream,
    )
    .await;

    if request.method == "initialize" {
        let client_capabilities =
            super::client_capabilities_from_initialize_params(request.params.as_ref());
        let response = router::route_request_with_config(handler, request, &ctx, config).await;

        // If successful and we have a session, extract and store the negotiated version.
        if let (Some(sid), Some(result)) = (session_id, response.result.as_ref())
            && let Some(version_str) = result.get("protocolVersion").and_then(|v| v.as_str())
        {
            let version = ProtocolVersion::from(version_str);
            session_manager
                .set_initialized(sid, version, client_capabilities)
                .await;
            tracing::debug!(
                session_id = sid,
                protocol_version = version_str,
                "Stored negotiated protocol version for session"
            );
        }

        return response;
    }

    // For post-initialize requests: use versioned routing if session has a stored version.
    if let Some(sid) = session_id
        && let Some(version) = session_manager.get_protocol_version(sid).await
    {
        return router::route_request_versioned(handler, request, &ctx, &version).await;
    }

    // Pre-initialize or sessionless: route with config for proper validation.
    router::route_request_with_config(handler, request, &ctx, config).await
}

/// Build the per-request context, installing a cancellation token when the
/// request belongs to a session and carries an id.
///
/// Returns the guard alongside the context; the caller must hold it for the
/// life of the dispatch so the registry entry is removed on every exit path.
async fn http_request_context(
    session_manager: &SessionManager,
    session_id: Option<&str>,
    request_id: Option<&serde_json::Value>,
    request_stream: Option<String>,
) -> (RequestContext, Option<PendingHandlerGuard>) {
    let mut ctx = RequestContext::http();
    let key = request_id.map(jsonrpc_id_key);

    if let Some(ref key) = key {
        ctx = ctx.with_request_id(key.clone());
    }

    let mut guard = None;
    if let Some(session_id) = session_id {
        let session = Arc::new(
            HttpSessionHandle::new(session_id.to_string(), session_manager.clone())
                .with_request_stream(request_stream),
        ) as Arc<dyn McpSession>;
        ctx = ctx
            .with_session_id(session_id.to_string())
            .with_session(session);

        // Only a request can be cancelled — a notification has no id for the
        // client to name in `notifications/cancelled`.
        if let Some(key) = key
            && let Some((token, pending_guard)) = session_manager
                .register_pending_handler(session_id, key)
                .await
        {
            ctx = ctx.with_cancellation_token(Arc::new(token) as Arc<dyn Cancellable>);
            guard = Some(pending_guard);
        }
    }

    (ctx, guard)
}

fn parse_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Walk the error source chain looking for `http_body_util::LengthLimitError`.
///
/// `axum::body::to_bytes` wraps the body in `http_body_util::Limited` which
/// emits a `LengthLimitError` with the documented `"length limit exceeded"`
/// display form when the payload exceeds the configured limit. We match on
/// the display string to avoid a direct dependency on `http-body-util`.
fn is_length_limit_error(err: &axum::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(current) = source {
        if current.to_string() == "length limit exceeded" {
            return true;
        }
        source = current.source();
    }
    false
}

fn session_header_value(session_id: &str) -> HeaderValue {
    HeaderValue::from_str(session_id)
        .unwrap_or_else(|_| HeaderValue::from_static("invalid-session"))
}

fn to_security_headers(headers: &HeaderMap) -> SecurityHeaders {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn extract_request_ip(
    headers: &HeaderMap,
    extensions: &axum::http::Extensions,
    config: Option<&ServerConfig>,
) -> Option<IpAddr> {
    let security_headers = to_security_headers(headers);
    let peer_ip = extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0.ip());

    match peer_ip {
        Some(peer) => {
            // Honour proxy headers only when the immediate peer is a trusted
            // reverse proxy. Direct clients get their real socket IP back,
            // so `X-Forwarded-For` smuggling can't bypass per-IP rate limits
            // or the loopback short-circuit in origin validation.
            let trusted = config
                .map(|c| c.origin_validation.trusted_proxies.as_slice())
                .unwrap_or(&[]);
            Some(extract_client_ip_with_trust(
                &security_headers,
                peer,
                trusted,
            ))
        }
        None => {
            // No `ConnectInfo` (e.g. tower::Service composed without it).
            // Fall back to header extraction, which is documented as
            // unsafe; callers in this state must trust the upstream layer.
            extract_client_ip(&security_headers)
        }
    }
}

fn origin_config(config: Option<&ServerConfig>) -> OriginConfig {
    let Some(config) = config else {
        return OriginConfig::default();
    };

    OriginConfig {
        allowed_origins: config.origin_validation.allowed_origins.clone(),
        allow_localhost: config.origin_validation.allow_localhost,
        allow_any: config.origin_validation.allow_any,
    }
}

fn validate_origin_header(
    headers: &HeaderMap,
    client_ip: Option<IpAddr>,
    config: Option<&ServerConfig>,
) -> Result<(), StatusCode> {
    let security_headers = to_security_headers(headers);
    let origin_config = origin_config(config);

    let client_ip = client_ip.unwrap_or(IpAddr::from([0, 0, 0, 0]));
    validate_origin(&origin_config, &security_headers, client_ip).map_err(|error| {
        tracing::warn!(%error, "Rejected HTTP request with invalid origin");
        StatusCode::FORBIDDEN
    })
}

fn json_response(status: StatusCode, body: JsonRpcOutgoing) -> Response {
    (status, axum::Json(body)).into_response()
}

fn empty_response(status: StatusCode) -> Response {
    status.into_response()
}

// SessionManager and sse_event_bytes are made `pub` (doc-hidden) only under
// the `internal-bench` feature via the cfg'd struct and fn definitions above/below.

#[allow(dead_code)]
fn sse_event_bytes_inner(id: &str, event_type: Option<&str>, data: &str) -> Bytes {
    // Pre-size the buffer so the per-message hot path performs a single
    // allocation instead of amplification-by-doubling as the frame grows.
    // Sized for the dominant single-line (JSON-RPC) case: "id: {id}\n" +
    // optional "event: {event_type}\n" + "data: {data}\n" + trailing "\n".
    // Multi-line data needs +6 bytes per extra line and may regrow; counting
    // lines up front to size exactly costs a full scan of `data`, which
    // benchmarked slower than the occasional regrowth it avoids.
    let capacity = 5 + id.len() + event_type.map_or(0, |t| t.len() + 8) + data.len() + 8;
    let mut event = String::with_capacity(capacity);
    event.push_str("id: ");
    event.push_str(id);
    event.push('\n');

    if let Some(event_type) = event_type {
        event.push_str("event: ");
        event.push_str(event_type);
        event.push('\n');
    }

    if data.is_empty() {
        event.push_str("data:\n");
    } else {
        for line in data.split('\n') {
            event.push_str("data: ");
            // Strip trailing \r from the split line to keep the wire format
            // clean (data may contain \r\n).
            event.push_str(line.strip_suffix('\r').unwrap_or(line));
            event.push('\n');
        }
    }

    event.push('\n');
    Bytes::from(event)
}

/// Frame one payload as an SSE event (`id:` / `event:` / `data:` lines).
///
/// Exposed under the `internal-bench` feature for `benches/sse_throughput.rs`;
/// treat as crate-internal otherwise.
#[cfg(not(feature = "internal-bench"))]
pub(crate) fn sse_event_bytes(id: &str, event_type: Option<&str>, data: &str) -> Bytes {
    sse_event_bytes_inner(id, event_type, data)
}

#[cfg(feature = "internal-bench")]
#[doc(hidden)]
pub fn sse_event_bytes(id: &str, event_type: Option<&str>, data: &str) -> Bytes {
    sse_event_bytes_inner(id, event_type, data)
}

fn validate_protocol_header(
    headers: &HeaderMap,
    config: Option<&ServerConfig>,
    expected: Option<&ProtocolVersion>,
) -> Result<(), StatusCode> {
    let Some(raw) = headers.get("mcp-protocol-version") else {
        // Per MCP 2025-11-25 §Streamable HTTP, post-init requests MUST carry
        // `Mcp-Protocol-Version`. Pre-init (no `expected` yet) is permissive
        // so that the very first POST `initialize` doesn't have to negotiate
        // a version it hasn't seen yet. Some deployed clients, including
        // Codex/rmcp 0.130, omit the header on `notifications/initialized`
        // even after a successful negotiation; tolerate the absence and keep
        // using the version already associated with this session.
        if expected.is_some() {
            tracing::debug!(
                "Post-init request missing Mcp-Protocol-Version header; continuing with session version"
            );
        }
        return Ok(());
    };

    let value = raw.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
    let version = ProtocolVersion::from(value);
    let protocol_config = config.map(|cfg| cfg.protocol.clone()).unwrap_or_default();

    if !protocol_config.is_supported(&version) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if let Some(expected) = expected
        && expected != &version
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

async fn resolve_session_for_request<H: McpHandler>(
    state: &SseState<H>,
    headers: &HeaderMap,
    method: &str,
) -> Result<Option<String>, StatusCode> {
    let session_id = parse_session_id(headers);

    if method == "initialize" {
        if session_id.is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        // A header sent here is still subject to validation. Passing `None`
        // keeps *absence* permissive — the first POST has no negotiated
        // version to state — while a present-but-unsupported value gets the
        // 400 the transport spec requires, which is the signal telling a
        // legacy client to fall back.
        validate_protocol_header(headers, state.config.as_ref(), None)?;
        return Ok(None);
    }

    // MCP 2025-11-25 lifecycle permits ping before the server has responded
    // to initialize. With no session yet, route it as a sessionless request.
    if method == "ping" && session_id.is_none() {
        validate_protocol_header(headers, state.config.as_ref(), None)?;
        return Ok(None);
    }

    let Some(session_id) = session_id else {
        return Err(StatusCode::BAD_REQUEST);
    };

    if !state.session_manager.has_session(&session_id).await {
        return Err(StatusCode::NOT_FOUND);
    }

    let expected = state
        .session_manager
        .get_protocol_version(&session_id)
        .await;
    validate_protocol_header(headers, state.config.as_ref(), expected.as_ref())?;

    Ok(Some(session_id))
}

async fn resolve_session_for_response<H: McpHandler>(
    state: &SseState<H>,
    headers: &HeaderMap,
) -> Result<String, StatusCode> {
    let Some(session_id) = parse_session_id(headers) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    if !state.session_manager.has_session(&session_id).await {
        return Err(StatusCode::NOT_FOUND);
    }

    let expected = state
        .session_manager
        .get_protocol_version(&session_id)
        .await;
    validate_protocol_header(headers, state.config.as_ref(), expected.as_ref())?;

    Ok(session_id)
}

async fn handle_client_json_rpc_response<H: McpHandler>(
    state: &SseState<H>,
    headers: &HeaderMap,
    response: CoreJsonRpcResponse,
) -> Response {
    let session_id = match resolve_session_for_response(state, headers).await {
        Ok(session_id) => session_id,
        Err(status) => return empty_response(status),
    };

    match state
        .session_manager
        .complete_pending_server_response(&session_id, response)
        .await
    {
        Ok(()) => empty_response(StatusCode::ACCEPTED),
        Err(status) => empty_response(status),
    }
}

/// Does the client accept an SSE answer to this POST?
///
/// §Sending Messages item 2 makes listing `text/event-stream` a client MUST, and
/// item 5 leaves the choice of form to the server. A client that omits it gets
/// the single-JSON-object form. `*/*` and `text/*` count: item 5 also obliges
/// the client to support both forms, so a blanket accept is an accept.
fn accepts_event_stream(headers: &HeaderMap) -> bool {
    let Some(accept) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) else {
        return false;
    };

    accept.split(',').any(|entry| {
        let mime = entry.split(';').next().unwrap_or("").trim();
        mime.eq_ignore_ascii_case("text/event-stream")
            || mime.eq_ignore_ascii_case("text/*")
            || mime == "*/*"
    })
}

/// Dispatch one request on its own task.
///
/// Spawning is what lets a handler panic be answered rather than dropped: an
/// unwind inside the axum handler propagates into hyper, which closes the
/// connection with no response at all, leaving a client without a per-request
/// timeout waiting forever. The other three transports already route through
/// `route_catching_panics`; this gives HTTP the same guarantee.
///
/// It also means a client disconnect no longer kills the handler mid-flight,
/// which is what §Sending Messages item 6 asks for: "Disconnection SHOULD NOT
/// be interpreted as the client cancelling its request." Cancellation is
/// explicit, via `notifications/cancelled`.
fn dispatch_request<H: McpHandler>(
    state: &SseState<H>,
    request: JsonRpcIncoming,
    session_id: Option<String>,
    request_stream: Option<String>,
) -> tokio::task::JoinHandle<JsonRpcOutgoing> {
    let handler = state.handler.clone();
    let session_manager = state.session_manager.clone();
    let config = state.config.clone();

    tokio::spawn(async move {
        route_with_version_tracking(
            &handler,
            request,
            &session_manager,
            config.as_ref(),
            session_id.as_deref(),
            request_stream,
        )
        .await
    })
}

/// Unwrap a dispatch task, reporting a panicking handler as a server fault.
///
/// The panic payload is logged in full but only summarised to the client, since
/// it can carry internal detail.
fn dispatch_outcome(
    outcome: Result<JsonRpcOutgoing, tokio::task::JoinError>,
    id: Option<serde_json::Value>,
    method: &str,
) -> JsonRpcOutgoing {
    match outcome {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(
                method = %method,
                %error,
                "Handler panicked; answering with an internal error"
            );
            JsonRpcOutgoing::error(
                id,
                McpError::internal(format!("Handler panicked while serving {method}")),
            )
        }
    }
}

/// Build the `text/event-stream` response around an already-built body.
fn sse_response(session_id: &str, body: Body) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .expect("SSE response builder should be valid");
    response
        .headers_mut()
        .insert("mcp-session-id", session_header_value(session_id));
    response
}

/// Answer a POST with an SSE stream, upgrading only if the handler needs one.
///
/// Runs the dispatch and races it against its own outbound traffic. A handler
/// that finishes without emitting anything gets the plain JSON answer it would
/// have got before; one that emits gets a stream carrying those messages and
/// then, per §Sending Messages item 6, the JSON-RPC response for the
/// originating request, after which the stream terminates.
async fn stream_json_rpc<H: McpHandler>(
    state: SseState<H>,
    request: JsonRpcIncoming,
    session_id: String,
) -> Response {
    let method = request.method.clone();
    let request_id = request.id.clone();

    // The stream is registered with the session up front so the handler can
    // address it by id, and so that a client whose connection drops mid-call
    // can resume it with `Last-Event-ID` — §Resumability applies "regardless of
    // how the original stream was initiated (via POST or GET)".
    let Some((stream_id, primer_id, mut stream_rx)) =
        state.session_manager.open_stream(&session_id, false).await
    else {
        return empty_response(StatusCode::NOT_FOUND);
    };

    let mut dispatch = dispatch_request(
        &state,
        request,
        Some(session_id.clone()),
        Some(stream_id.clone()),
    );

    let unstreamed = |outcome| {
        let response = dispatch_outcome(outcome, request_id.clone(), &method);
        if response.should_send() {
            json_response(StatusCode::OK, response)
        } else {
            empty_response(StatusCode::ACCEPTED)
        }
    };

    let first = tokio::select! {
        biased;
        first = stream_rx.recv() => first,
        outcome = &mut dispatch => {
            state.session_manager.close_stream(&session_id, &stream_id).await;
            return unstreamed(outcome);
        }
    };

    let Some(first) = first else {
        // The channel can only close with the session, which takes the stream
        // entry with it.
        return unstreamed(dispatch.await);
    };

    let session_manager = state.session_manager.clone();
    let stream_session_id = session_id.clone();
    let stream = async_stream::stream! {
        // §Sending Messages item 6: "The server SHOULD immediately send an SSE
        // event consisting of an event ID and an empty `data` field in order to
        // prime the client to reconnect." The `: connected` comment goes first
        // for the same reason as on the GET stream — older RMCP/Codex clients
        // misparse a bare `data:\n\n` as a JSON-RPC payload.
        yield Ok::<_, std::convert::Infallible>(Bytes::from_static(b": connected\n\n"));
        yield Ok::<_, std::convert::Infallible>(sse_event_bytes(&primer_id, None, ""));
        yield Ok::<_, std::convert::Infallible>(
            sse_event_bytes(&first.0, Some("message"), &first.1),
        );

        let outcome = loop {
            tokio::select! {
                biased;
                message = stream_rx.recv() => match message {
                    Some((id, data)) => yield Ok::<_, std::convert::Infallible>(
                        sse_event_bytes(&id, Some("message"), &data),
                    ),
                    None => break dispatch.await,
                },
                outcome = &mut dispatch => break outcome,
            }
        };

        // The response goes through the session manager like any other event so
        // it gets this stream's next id and is retained for replay: a client
        // that dropped before receiving it can reconnect and still collect it.
        let response = dispatch_outcome(outcome, request_id, &method);
        if response.should_send()
            && let Ok(payload) = serde_json::to_string(&response)
        {
            session_manager
                .send_to_stream(&stream_session_id, &stream_id, &payload)
                .await;
        }

        // Drain in arrival order. Anything the handler queued in the instant
        // before it returned still belongs ahead of the response: progress that
        // trails the result it describes is worse than no progress at all.
        while let Ok((id, data)) = stream_rx.try_recv() {
            yield Ok::<_, std::convert::Infallible>(
                sse_event_bytes(&id, Some("message"), &data),
            );
        }
        // §Sending Messages item 6: "After the JSON-RPC response has been sent,
        // the server SHOULD terminate the SSE stream."
    };

    sse_response(&session_id, Body::from_stream(stream))
}

/// Axum handler for JSON-RPC requests (simple mode).
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    request: axum::http::Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let client_ip = extract_request_ip(&headers, &parts.extensions, state.config.as_ref());
    if let Err(status) = validate_origin_header(&headers, client_ip, state.config.as_ref()) {
        return empty_response(status);
    }

    if let Some(ref limiter) = state.rate_limiter {
        let client_id = client_ip.map(|ip| ip.to_string());
        if !limiter.check(client_id.as_deref()) {
            tracing::warn!("Rate limit exceeded for HTTP client");
            return empty_response(StatusCode::TOO_MANY_REQUESTS);
        }
    }

    // Reject oversized bodies with 413 Payload Too Large rather than 400 so
    // clients can tell "body is malformed" from "body too big to accept".
    // Prefer the Content-Length header as a fast, stream-free check, then
    // fall back to inspecting the to_bytes error chain for chunked bodies.
    let max_body_size = state
        .config
        .as_ref()
        .map_or(MAX_BODY_SIZE, |config| config.max_message_size);
    if let Some(declared_len) = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        && declared_len > max_body_size
    {
        return empty_response(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let payload = match to_bytes(body, max_body_size).await {
        Ok(body) => match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(payload) => payload,
            Err(_) => return empty_response(StatusCode::BAD_REQUEST),
        },
        Err(err) => {
            let status = if is_length_limit_error(&err) {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            return empty_response(status);
        }
    };

    if let Ok(response) = serde_json::from_value::<CoreJsonRpcResponse>(payload.clone()) {
        return handle_client_json_rpc_response(&state, &headers, response).await;
    }

    let request = match serde_json::from_value::<JsonRpcIncoming>(payload) {
        Ok(request) => request,
        Err(_) => return empty_response(StatusCode::BAD_REQUEST),
    };
    let is_initialize = request.method == "initialize";
    let client_capabilities = if is_initialize {
        Some(super::client_capabilities_from_initialize_params(
            request.params.as_ref(),
        ))
    } else {
        None
    };
    let session_id = match resolve_session_for_request(&state, &headers, &request.method).await {
        Ok(session_id) => session_id,
        Err(status) => return empty_response(status),
    };

    // No request-id uniqueness check here. The spec's "MUST NOT have been
    // previously used" binds the *requestor*; a receiver's only obligation is
    // to echo the id back, so rejecting a reused one bought nothing and cost an
    // unbounded per-session set that never shrank. Real clients reuse ids
    // across a long-lived session and were locked out (#25).

    // MCP §Cancellation: signal the matching in-flight handler. Consumed here
    // rather than routed, exactly as line / channel / websocket do — the core
    // router has no handle on any registry, and the registry is inherently
    // per-session transport state.
    if request.method == "notifications/cancelled" {
        if let (Some(session_id), Some(cancelled_id)) = (
            session_id.as_deref(),
            request.params.as_ref().and_then(|p| p.get("requestId")),
        ) {
            let key = jsonrpc_id_key(cancelled_id);
            let reason = request
                .params
                .as_ref()
                .and_then(|p| p.get("reason"))
                .and_then(|r| r.as_str())
                .unwrap_or("client requested cancellation");
            if state
                .session_manager
                .cancel_pending_handler(session_id, &key)
                .await
            {
                tracing::debug!(
                    request_id = %key,
                    reason = %reason,
                    "Cancelling in-flight handler",
                );
            }
        }
        return empty_response(StatusCode::ACCEPTED);
    }

    // §Sending Messages item 5: a JSON-RPC request may be answered with either
    // a single JSON object or an SSE stream, and item 6 says request-related
    // server messages ride that stream. Until 3.5.0 this server only ever
    // answered with JSON, so everything a handler emitted mid-request —
    // `ctx.sample()`, elicitation, progress — was pushed onto the standalone
    // GET stream, which item 4 of §Listening for Messages reserves for
    // *unrelated* messages. A client that never issued a GET (Inspector's
    // stateless mode, curl harnesses, serverless callers) got
    // `-32603 No active SSE stream` from every sample and silently lost every
    // progress notification.
    //
    // The upgrade is lazy: JSON stays the answer unless the handler actually
    // emits something, so the common request/response case is unchanged.
    if request.id.is_some()
        && !is_initialize
        && let Some(session_id) = session_id.clone()
        && accepts_event_stream(&headers)
    {
        return stream_json_rpc(state, request, session_id).await;
    }

    let initialize_request_id = request.id.clone();
    let method = request.method.clone();
    let request_id = request.id.clone();
    let response = dispatch_outcome(
        dispatch_request(&state, request, session_id.clone(), None).await,
        request_id,
        &method,
    );

    if !response.should_send() {
        return empty_response(StatusCode::ACCEPTED);
    }

    if is_initialize
        && let Some(result) = response.result.as_ref()
        && let Some(version_str) = result.get("protocolVersion").and_then(|v| v.as_str())
    {
        let session_id = state
            .session_manager
            .create_session(initialize_request_id.as_ref())
            .await;
        state
            .session_manager
            .set_initialized(
                &session_id,
                ProtocolVersion::from(version_str),
                client_capabilities.unwrap_or_default(),
            )
            .await;

        let mut response = json_response(StatusCode::OK, response);
        response
            .headers_mut()
            .insert("mcp-session-id", session_header_value(&session_id));
        return response;
    }

    json_response(StatusCode::OK, response)
}

/// Axum handler for SSE (Server-Sent Events) connections.
///
/// This implements the MCP Streamable HTTP specification:
/// - Returns `text/event-stream` content type
/// - Sets `Mcp-Session-Id` header for session correlation
/// - Keeps connection open for server-initiated messages
async fn handle_sse<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    request: axum::http::Request<Body>,
) -> Response {
    let (parts, _) = request.into_parts();
    let headers = parts.headers;
    let client_ip = extract_request_ip(&headers, &parts.extensions, state.config.as_ref());
    if let Err(status) = validate_origin_header(&headers, client_ip, state.config.as_ref()) {
        return empty_response(status);
    }

    let session_id = match parse_session_id(&headers) {
        Some(session_id) => session_id,
        None => return empty_response(StatusCode::BAD_REQUEST),
    };
    if !state.session_manager.has_session(&session_id).await {
        return empty_response(StatusCode::NOT_FOUND);
    }
    let expected = state
        .session_manager
        .get_protocol_version(&session_id)
        .await;
    if validate_protocol_header(&headers, state.config.as_ref(), expected.as_ref()).is_err() {
        return empty_response(StatusCode::BAD_REQUEST);
    }
    // §Resumability: a client that lost its connection reconnects by GET with
    // `Last-Event-ID`, and the server replays what that *same* stream sent
    // after it. Event IDs are `{session}-{stream}-{seq}` for exactly this: the
    // id identifies its originating stream, so the correlation is possible.
    // Anything that does not resolve — malformed, unknown stream, another
    // session's id — falls through to a fresh stream rather than erroring, the
    // same answer the client would have got without the header.
    let resumed = match headers.get("last-event-id").and_then(|v| v.to_str().ok()) {
        Some(last_event_id) => {
            state
                .session_manager
                .resume_stream(&session_id, last_event_id)
                .await
        }
        None => None,
    };

    let (primer, replay, mut rx) = match resumed {
        // No primer on a resume: the client already holds an anchor, and the
        // replayed events carry their original ids.
        Some((_, replay, rx)) => (None, replay, rx),
        None => match state.session_manager.open_stream(&session_id, true).await {
            Some((_, primer_id, rx)) => (Some(primer_id), Vec::new(), rx),
            None => return empty_response(StatusCode::NOT_FOUND),
        },
    };

    let stream = async_stream::stream! {
        // Send the `: connected` comment first so older RMCP/Codex clients that
        // misparse `data:\n\n` as a JSON-RPC payload are unaffected.
        yield Ok::<_, std::convert::Infallible>(Bytes::from_static(b": connected\n\n"));
        if let Some(primer_id) = primer {
            // §Resumability permits attaching event IDs; doing so hands the
            // client an immediate `Last-Event-ID` anchor to resume from. The
            // primer takes seq 0, so messages start at 1.
            yield Ok::<_, std::convert::Infallible>(sse_event_bytes(&primer_id, None, ""));
        }

        for (id, data) in replay {
            yield Ok::<_, std::convert::Infallible>(
                sse_event_bytes(&id, Some("message"), &data),
            );
        }

        // Drain messages routed to this specific stream. Per §Multiple
        // Connections we only see messages the server explicitly chose to send
        // here; other concurrent streams on the same session have their own
        // receivers.
        loop {
            match tokio::time::timeout(Duration::from_secs(SSE_KEEP_ALIVE_SECS), rx.recv()).await {
                Ok(Some((id, data))) => {
                    yield Ok::<_, std::convert::Infallible>(
                        sse_event_bytes(&id, Some("message"), &data),
                    );
                }
                Ok(None) => {
                    tracing::debug!("SSE subscriber channel closed");
                    break;
                }
                Err(_) => {
                    yield Ok::<_, std::convert::Infallible>(Bytes::from_static(b": keep-alive\n\n"));
                }
            }
        }
    };

    sse_response(&session_id, Body::from_stream(stream))
}

/// Explicitly terminate an HTTP session.
async fn handle_delete_session<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    request: axum::http::Request<Body>,
) -> Response {
    let (parts, _) = request.into_parts();
    let headers = parts.headers;
    let client_ip = extract_request_ip(&headers, &parts.extensions, state.config.as_ref());
    if let Err(status) = validate_origin_header(&headers, client_ip, state.config.as_ref()) {
        return empty_response(status);
    }

    let Some(session_id) = parse_session_id(&headers) else {
        return empty_response(StatusCode::BAD_REQUEST);
    };

    if !state.session_manager.has_session(&session_id).await {
        return empty_response(StatusCode::NOT_FOUND);
    }

    let expected = state
        .session_manager
        .get_protocol_version(&session_id)
        .await;
    if validate_protocol_header(&headers, state.config.as_ref(), expected.as_ref()).is_err() {
        return empty_response(StatusCode::BAD_REQUEST);
    }

    if state.session_manager.remove_session(&session_id).await {
        return empty_response(StatusCode::NO_CONTENT);
    }

    empty_response(StatusCode::NOT_FOUND)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tower::ServiceExt;
    use turbomcp_core::context::RequestContext as CoreRequestContext;
    use turbomcp_types::{
        Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
    };

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            Vec::new()
        }

        fn list_resources(&self) -> Vec<Resource> {
            Vec::new()
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            Vec::new()
        }

        async fn call_tool(
            &self,
            name: &str,
            _args: Value,
            _ctx: &CoreRequestContext,
        ) -> McpResult<ToolResult> {
            Err(McpError::tool_not_found(name))
        }

        async fn read_resource(
            &self,
            uri: &str,
            _ctx: &CoreRequestContext,
        ) -> McpResult<ResourceResult> {
            Err(McpError::resource_not_found(uri))
        }

        async fn get_prompt(
            &self,
            name: &str,
            _args: Option<Value>,
            _ctx: &CoreRequestContext,
        ) -> McpResult<PromptResult> {
            Err(McpError::prompt_not_found(name))
        }
    }

    #[test]
    fn sse_event_bytes_formats_multiline_data_and_strips_crlf() {
        let event = sse_event_bytes("session-stream-1", Some("message"), "one\r\ntwo\nthree");
        let text = std::str::from_utf8(event.as_ref()).expect("valid utf8");

        assert_eq!(
            text,
            "id: session-stream-1\nevent: message\ndata: one\ndata: two\ndata: three\n\n"
        );
    }

    #[test]
    fn sse_event_bytes_formats_empty_data_without_retry() {
        let event = sse_event_bytes("session-stream-1", None, "");
        let text = std::str::from_utf8(event.as_ref()).expect("valid utf8");

        assert_eq!(text, "id: session-stream-1\ndata:\n\n");
        assert!(!text.contains("retry:"));
    }

    // MCP 2025-11-25 §Multiple Connections:
    //   "The server MUST send each of its JSON-RPC messages on only one of
    //    the connected streams; that is, it MUST NOT broadcast the same
    //    message across multiple streams."
    //
    // The SessionManager must therefore keep every message on exactly one
    // of the session's subscribers even when multiple SSE streams are open
    // for that session.
    #[tokio::test]
    async fn send_to_session_routes_to_single_subscriber() {
        let manager = SessionManager::new();
        let session_id = manager.create_session(None).await;

        let mut rx1 = manager
            .subscribe_session(&session_id)
            .await
            .expect("first subscribe");
        let mut rx2 = manager
            .subscribe_session(&session_id)
            .await
            .expect("second subscribe");

        assert!(manager.send_to_session(&session_id, "hello").await);

        let first = tokio::time::timeout(std::time::Duration::from_millis(100), rx1.recv()).await;
        let second = tokio::time::timeout(std::time::Duration::from_millis(100), rx2.recv()).await;

        let first_got = matches!(first, Ok(Some((_, ref data))) if &**data == "hello");
        let second_got = matches!(second, Ok(Some((_, ref data))) if &**data == "hello");

        assert!(
            first_got ^ second_got,
            "message must reach exactly one subscriber, got first={first:?}, second={second:?}"
        );
    }

    #[tokio::test]
    async fn build_router_uses_configured_http_body_limit() {
        let config = ServerConfig::builder()
            .max_message_size(1024)
            .allow_any_origin(true)
            .build();
        let app = build_router(TestHandler, None, Some(config));
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from("x".repeat(2048)))
            .expect("request");

        let response = app.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    // HTTP route-level tests live in /tests/ because they need a bound port.
}
