//! Streamable HTTP transport for WASM MCP servers.
//!
//! This module implements the MCP Streamable HTTP transport (2025-06-18 and
//! 2025-11-25) for Cloudflare Workers and other edge runtimes.
//!
//! ## Features
//!
//! - **POST**: Send JSON-RPC messages; requests are answered with JSON,
//!   notifications and client responses with `202 Accepted`
//! - **GET**: SSE stream for server-initiated messages, replaying stored events
//! - **DELETE**: Terminate session
//! - **Session Management**: `Mcp-Session-Id`, issued on a successful
//!   `initialize` and required on every later request
//! - **Protocol version**: the version negotiated at `initialize` is stored
//!   with the session; `MCP-Protocol-Version` must match it, and responses are
//!   stepped down to it
//! - **Message Replay**: `Last-Event-ID` support for resumability
//! - **Origin Validation**: DNS rebinding protection, on by default
//!
//! ## Security
//!
//! ### Origin Validation
//!
//! A request whose `Origin` is present but neither loopback nor listed in
//! `StreamableConfig::allowed_origins` is refused with `403` before its body
//! is read — the same rule as the native HTTP transport. Requests without an
//! `Origin` (non-browser clients) pass unless `require_origin` is set. List
//! the origins of browser applications that call the server:
//!
//! ```ignore
//! let config = StreamableConfig::production().allow_origin("https://app.example.com");
//! ```
//!
//! ### CORS Handling
//!
//! The request `Origin` is echoed rather than answered with `*`, with
//! `Vary: Origin`; a refused origin is granted nothing.
//!
//! ### SSE on Workers
//!
//! A Worker cannot hold a response open to push messages later, so a GET
//! returns the stored events the client has not yet seen and ends the stream.
//! It carries a `retry` interval and an event id the client resumes from, so
//! a reconnecting client polls for new events rather than missing them.
//!
//! ## Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//! use turbomcp_wasm::wasm_server::streamable::*;
//!
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool("hello", "Say hello", hello_handler)
//!     .build();
//!
//! let streamable = StreamableHandler::new(server)
//!     .with_session_store(MemorySessionStore::new())
//!     .with_config(StreamableConfig::production());
//!
//! // In your Worker fetch handler:
//! streamable.handle(req).await
//! ```

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use turbomcp_core::handler::McpHandler;
use turbomcp_transport_streamable::{
    Session, SessionId, SessionStore, SseEncoder, SseEvent, StoredEvent,
};

/// Streamable HTTP configuration: timeouts, limits and origin rules.
pub use turbomcp_transport_streamable::StreamableConfig;
use turbomcp_types::ProtocolVersion;
use worker::{Request, Response};

use super::context::current_timestamp_ms;
use super::endpoint::{
    self, HeaderMap, Inbound, PROTOCOL_VERSION_HEADER, Reply, check_origin, is_json_content_type,
    negotiated_version, protocol_version_from_header, request_context,
};
use super::server::McpServer;

/// Methods this endpoint answers, for `Allow` and CORS.
const ALLOWED_METHODS: &str = "GET, POST, DELETE, OPTIONS";

/// Default cap on concurrently stored sessions for [`MemorySessionStore`].
const DEFAULT_MAX_SESSIONS: usize = 10_000;

/// In-memory session store.
///
/// Sessions and their event logs live in the Worker isolate's memory, so they
/// do not survive an isolate restart and are not shared between isolates; use
/// `DurableObjectSessionStore` for that.
///
/// The store is bounded. It holds at most `max_sessions` sessions — creating
/// one past the cap evicts the least recently active — and keeps the newest
/// `max_events_per_session` events per session as a ring buffer. Sessions idle
/// longer than the configured timeout are swept when new ones are created.
#[derive(Clone)]
pub struct MemorySessionStore {
    state: Arc<Mutex<MemoryState>>,
    max_sessions: usize,
    max_events_per_session: usize,
}

#[derive(Default)]
struct MemoryState {
    sessions: HashMap<String, Session>,
    events: HashMap<String, VecDeque<StoredEvent>>,
}

impl MemorySessionStore {
    /// Create a new in-memory session store with the default limits: 10 000
    /// sessions and [`StreamableConfig::default`]'s events per session.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryState::default())),
            max_sessions: DEFAULT_MAX_SESSIONS,
            max_events_per_session: StreamableConfig::default().max_events_per_session,
        }
    }

    /// Create a store that keeps `config.max_events_per_session` events per
    /// session.
    pub fn from_config(config: &StreamableConfig) -> Self {
        Self::new().with_max_events_per_session(config.max_events_per_session)
    }

    /// Cap the number of stored sessions.
    #[must_use]
    pub fn with_max_sessions(mut self, max_sessions: usize) -> Self {
        self.max_sessions = max_sessions.max(1);
        self
    }

    /// Cap the number of events kept per session for replay.
    #[must_use]
    pub fn with_max_events_per_session(mut self, max_events: usize) -> Self {
        self.max_events_per_session = max_events;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        // A panic while holding the lock leaves plain maps behind; nothing in
        // them can be half-updated in a way that matters, so keep serving.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for MemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore for MemorySessionStore {
    type Error = std::convert::Infallible;

    async fn create(&self) -> Result<SessionId, Self::Error> {
        let id = SessionId::new();
        // `Session::new` reads `SystemTime`, which panics on wasm32; the
        // timestamp comes from the JS clock instead.
        let session = Session::new_with_timestamp(id.clone(), current_timestamp_ms());
        let mut state = self.lock();
        while state.sessions.len() >= self.max_sessions {
            let Some(oldest) = state
                .sessions
                .values()
                .min_by_key(|session| session.last_activity)
                .map(|session| session.id.as_str().to_string())
            else {
                break;
            };
            state.sessions.remove(&oldest);
            state.events.remove(&oldest);
        }
        state.sessions.insert(id.as_str().to_string(), session);
        state
            .events
            .insert(id.as_str().to_string(), VecDeque::new());
        Ok(id)
    }

    async fn get(&self, id: &SessionId) -> Result<Option<Session>, Self::Error> {
        Ok(self.lock().sessions.get(id.as_str()).cloned())
    }

    async fn update(&self, session: &Session) -> Result<(), Self::Error> {
        let mut state = self.lock();
        // Updating a session that has since been destroyed must not bring it
        // back.
        if let Some(stored) = state.sessions.get_mut(session.id.as_str()) {
            *stored = session.clone();
        }
        Ok(())
    }

    async fn store_event(&self, id: &SessionId, event: StoredEvent) -> Result<(), Self::Error> {
        // Unknown sessions are a silent no-op: `Error = Infallible` leaves no
        // way to report it, and an event for a session that is gone has no
        // one to be replayed to.
        let mut state = self.lock();
        if let Some(events) = state.events.get_mut(id.as_str()) {
            if self.max_events_per_session == 0 {
                return Ok(());
            }
            while events.len() >= self.max_events_per_session {
                events.pop_front();
            }
            events.push_back(event);
        }
        Ok(())
    }

    async fn replay_from(
        &self,
        id: &SessionId,
        last_event_id: &str,
    ) -> Result<Vec<StoredEvent>, Self::Error> {
        let state = self.lock();
        let Some(events) = state.events.get(id.as_str()) else {
            return Ok(Vec::new());
        };
        // An id this session never issued — forged, from another session, or
        // aged out of the ring buffer — replays nothing. Replaying the whole
        // log instead handed a client every stored event, including ones it
        // had already processed.
        Ok(events
            .iter()
            .position(|event| event.id == last_event_id)
            .map(|index| events.iter().skip(index + 1).cloned().collect())
            .unwrap_or_default())
    }

    async fn destroy(&self, id: &SessionId) -> Result<(), Self::Error> {
        let mut state = self.lock();
        state.sessions.remove(id.as_str());
        state.events.remove(id.as_str());
        Ok(())
    }

    async fn cleanup_expired(&self, timeout_ms: u64) -> Result<u64, Self::Error> {
        let now = current_timestamp_ms();
        let mut state = self.lock();
        let expired: Vec<String> = state
            .sessions
            .values()
            .filter(|session| session.is_expired(now, timeout_ms))
            .map(|session| session.id.as_str().to_string())
            .collect();
        for id in &expired {
            state.sessions.remove(id);
            state.events.remove(id);
            super::rich_context::cleanup_session_state(id);
        }
        Ok(expired.len() as u64)
    }
}

/// Streamable HTTP handler for MCP servers.
///
/// Wraps any [`McpHandler`] — an [`McpServer`] by default, or a middleware
/// stack, visibility layer or composite — and serves it over the Streamable
/// HTTP transport: GET/POST/DELETE, sessions, SSE replay.
pub struct StreamableHandler<S: SessionStore = MemorySessionStore, H: McpHandler = McpServer> {
    handler: H,
    session_store: S,
    config: StreamableConfig,
    allow_any_origin: bool,
    #[cfg(target_arch = "wasm32")]
    event_sequence: std::cell::RefCell<u64>,
    #[cfg(not(target_arch = "wasm32"))]
    event_sequence: std::sync::atomic::AtomicU64,
}

impl<H: McpHandler> StreamableHandler<MemorySessionStore, H> {
    /// Create a new streamable handler with in-memory session storage.
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            session_store: MemorySessionStore::new(),
            config: StreamableConfig::default(),
            allow_any_origin: false,
            #[cfg(target_arch = "wasm32")]
            event_sequence: std::cell::RefCell::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            event_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl<S: SessionStore, H: McpHandler> StreamableHandler<S, H> {
    /// Create a new streamable handler with a custom session store.
    pub fn with_session_store<NewS: SessionStore>(
        self,
        session_store: NewS,
    ) -> StreamableHandler<NewS, H> {
        StreamableHandler {
            handler: self.handler,
            session_store,
            config: self.config,
            allow_any_origin: self.allow_any_origin,
            #[cfg(target_arch = "wasm32")]
            event_sequence: std::cell::RefCell::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            event_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Set the configuration.
    ///
    /// Session timeouts, the body limit and the origin rules take effect here.
    /// The in-memory store keeps its own event limit; build it with
    /// [`MemorySessionStore::from_config`] to match `max_events_per_session`.
    pub fn with_config(mut self, config: StreamableConfig) -> Self {
        self.config = config;
        self
    }

    /// Disable origin validation entirely.
    ///
    /// Only for endpoints that are unreachable from browsers or authenticate
    /// every request; everything else should list its origins in
    /// `StreamableConfig::allowed_origins`.
    #[must_use]
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.allow_any_origin = allow;
        self
    }

    /// Handle an incoming HTTP request.
    ///
    /// Routes to the appropriate handler based on HTTP method:
    /// - GET: SSE stream (replay of stored events)
    /// - POST: JSON-RPC message
    /// - DELETE: Terminate session
    /// - OPTIONS: CORS preflight
    ///
    /// Any other method is answered `405`.
    pub async fn handle(&self, mut req: Request) -> worker::Result<Response> {
        let headers = endpoint::header_map(&req);
        let origin = headers.get("origin").cloned();
        let method = req.method().to_string().to_ascii_uppercase();

        let reply = match self.precheck(&method, &headers) {
            Err(reply) => reply,
            Ok(()) => {
                let body = if method == "POST" {
                    endpoint::read_body(&mut req, self.config.max_body_size)
                        .await
                        .map(Some)
                } else {
                    Ok(None)
                };
                match body {
                    Err(reply) => reply,
                    Ok(body) => self.process(&method, &headers, body.as_deref()).await,
                }
            }
        };
        endpoint::into_response(reply, origin.as_deref(), ALLOWED_METHODS)
    }

    /// Checks that need only the method and headers, so a refused request is
    /// turned away before its body is read.
    fn precheck(&self, method: &str, headers: &HeaderMap) -> Result<(), Reply> {
        if !matches!(method, "GET" | "POST" | "DELETE" | "OPTIONS") {
            return Err(Reply::method_not_allowed(ALLOWED_METHODS));
        }

        let origin = headers.get("origin").map(String::as_str);
        if origin.is_none() && self.config.require_origin && !self.allow_any_origin {
            return Err(Reply::text(403, "Origin header required"));
        }
        check_origin(
            origin,
            &self.config.allowed_origins,
            true,
            self.allow_any_origin,
        )
        .map_err(|reason| Reply::text(403, reason))?;

        if method == "OPTIONS" {
            return Err(Reply::empty(204));
        }
        if method == "POST" {
            if !is_json_content_type(headers.get("content-type").map(String::as_str)) {
                return Err(Reply::text(
                    415,
                    "Unsupported Media Type. Use Content-Type: application/json",
                ));
            }
            if headers
                .get("content-length")
                .and_then(|length| length.parse::<usize>().ok())
                .is_some_and(|length| length > self.config.max_body_size)
            {
                return Err(Reply::text(413, "Request body too large"));
            }
        }
        Ok(())
    }

    /// Answer a request that passed [`Self::precheck`].
    async fn process(&self, method: &str, headers: &HeaderMap, body: Option<&str>) -> Reply {
        match method {
            "GET" => self.handle_get(headers).await,
            "POST" => self.handle_post(headers, body.unwrap_or_default()).await,
            "DELETE" => self.handle_delete(headers).await,
            _ => Reply::method_not_allowed(ALLOWED_METHODS),
        }
    }

    /// Handle POST: one JSON-RPC message.
    async fn handle_post(&self, headers: &HeaderMap, body: &str) -> Reply {
        if !accepts(headers, "application/json") {
            return Reply::text(406, "Accept must allow application/json");
        }
        let header_version = match protocol_version_from_header(
            headers.get(PROTOCOL_VERSION_HEADER).map(String::as_str),
        ) {
            Ok(version) => version,
            Err(reason) => return Reply::text(400, reason),
        };

        let request = match endpoint::parse_message(body) {
            Inbound::Message(request) => Some(request),
            Inbound::ClientResponse => None,
            Inbound::Invalid(error) => return Reply::rpc(400, &error),
        };

        if request.as_ref().is_some_and(|r| r.method == "initialize") {
            return self
                .initialize(headers, request.expect("checked above"))
                .await;
        }

        let mut ctx = request_context(headers);
        let mut version = header_version;

        // `ping` may precede initialization, so it is answered without a
        // session, as the native transport does.
        let sessionless_ping = request.as_ref().is_some_and(|r| r.method == "ping")
            && !headers.contains_key("mcp-session-id");

        if self.config.enable_sessions && !sessionless_ping {
            let session = match self.resolve_session(headers).await {
                Ok(session) => session,
                Err(reply) => return reply,
            };
            version = session
                .protocol_version
                .as_deref()
                .map(ProtocolVersion::from);
            ctx = ctx.with_session_id(session.id.as_str());
        }

        let Some(request) = request else {
            // A client's response to a server request. This server sends
            // none, so there is nothing to deliver it to; the transport spec
            // still asks for 202.
            return Reply::empty(202);
        };

        let response = endpoint::route(&self.handler, request, &ctx, version.as_ref()).await;
        if response.should_send() {
            Reply::rpc(200, &response)
        } else {
            Reply::empty(202)
        }
    }

    /// Handle `initialize`, issuing a session when it succeeds.
    async fn initialize(
        &self,
        headers: &HeaderMap,
        request: turbomcp_core::jsonrpc::JsonRpcIncoming,
    ) -> Reply {
        // A client re-initializing inside a session is confused about which
        // session it is in; refuse rather than guess.
        if headers.contains_key("mcp-session-id") {
            return Reply::text(400, "initialize must not carry Mcp-Session-Id");
        }

        let ctx = request_context(headers);
        let response = endpoint::route(&self.handler, request, &ctx, None).await;
        if !response.should_send() {
            return Reply::empty(202);
        }

        // Only a successful handshake gets a session: creating one first left
        // a session behind for every failed or malformed initialize.
        let session_id = match (self.config.enable_sessions, negotiated_version(&response)) {
            (true, Some(version)) => match self.open_session(&version).await {
                Ok(id) => Some(id),
                Err(reply) => return reply,
            },
            _ => None,
        };

        let reply = Reply::rpc(200, &response);
        match session_id {
            Some(id) => reply.with_header("Mcp-Session-Id", id.into_string()),
            None => reply,
        }
    }

    /// Create a session and record the version negotiated for it.
    async fn open_session(&self, version: &ProtocolVersion) -> Result<SessionId, Reply> {
        // Sweep abandoned sessions first; stores with their own expiry (such
        // as Durable Objects) treat this as a no-op.
        let _ = self
            .session_store
            .cleanup_expired(self.config.idle_timeout_ms)
            .await;

        let id = self
            .session_store
            .create()
            .await
            .map_err(|_| Reply::text(500, "Failed to create session"))?;
        let mut session = match self.session_store.get(&id).await {
            Ok(Some(session)) => session,
            _ => return Err(Reply::text(500, "Failed to create session")),
        };
        session.protocol_version = Some(version.as_str().to_string());
        session.activate();
        session.touch_with_timestamp(current_timestamp_ms());
        self.session_store
            .update(&session)
            .await
            .map_err(|_| Reply::text(500, "Failed to create session"))?;
        Ok(id)
    }

    /// Look up the session a request names and hold the request to it.
    ///
    /// `400` when the header is missing or malformed, `404` when the session
    /// is unknown, terminated or expired (the transport spec's signal to start
    /// a new one), and `400` when `MCP-Protocol-Version` is present but is not
    /// the version negotiated for the session.
    async fn resolve_session(&self, headers: &HeaderMap) -> Result<Session, Reply> {
        let Some(raw) = headers.get("mcp-session-id") else {
            return Err(Reply::text(400, "Mcp-Session-Id header required"));
        };
        let Some(id) = SessionId::try_from_string(raw.clone()) else {
            return Err(Reply::text(400, "Invalid Mcp-Session-Id header"));
        };

        let mut session = match self.session_store.get(&id).await {
            Ok(Some(session)) => session,
            Ok(None) => return Err(Reply::text(404, "Session not found")),
            Err(_) => return Err(Reply::text(500, "Session store error")),
        };

        let now = current_timestamp_ms();
        let expired = session.is_expired(now, self.config.idle_timeout_ms)
            || now.saturating_sub(session.created_at) > self.config.session_timeout_ms;
        if !session.can_accept_requests() || expired {
            self.end_session(&id).await;
            return Err(Reply::text(404, "Session not found"));
        }

        if let Some(requested) = headers.get(PROTOCOL_VERSION_HEADER)
            && let Some(negotiated) = session.protocol_version.as_deref()
            && requested.trim() != negotiated
        {
            return Err(Reply::text(
                400,
                format!(
                    "MCP-Protocol-Version '{requested}' does not match the session's '{negotiated}'"
                ),
            ));
        }

        session.touch_with_timestamp(now);
        let _ = self.session_store.update(&session).await;
        Ok(session)
    }

    /// Forget a session and everything kept for it.
    async fn end_session(&self, id: &SessionId) {
        let _ = self.session_store.destroy(id).await;
        super::rich_context::cleanup_session_state(id.as_str());
    }

    /// Handle GET: an SSE stream of the events the client has not seen.
    async fn handle_get(&self, headers: &HeaderMap) -> Reply {
        if !self.config.enable_sessions {
            // Without sessions there is nothing to stream; 405 is how the
            // transport spec says "no SSE stream at this endpoint".
            return Reply::method_not_allowed("POST, OPTIONS");
        }
        if !accepts(headers, "text/event-stream") {
            return Reply::text(406, "Accept must allow text/event-stream");
        }
        let session = match self.resolve_session(headers).await {
            Ok(session) => session,
            Err(reply) => return reply,
        };

        let mut body =
            String::from_utf8_lossy(&SseEncoder::encode_comment("connected")).into_owned();
        body.push_str(&format!("retry: {}\n\n", self.config.retry_interval_ms));

        match headers.get("last-event-id") {
            Some(last_event_id) => {
                let events = self
                    .session_store
                    .replay_from(&session.id, last_event_id)
                    .await
                    .unwrap_or_default();
                for event in events.into_iter().filter(|event| !event.data.is_empty()) {
                    let mut sse = SseEvent::with_id(event.id, event.data);
                    sse.event = event.event_type;
                    body.push_str(&SseEncoder::encode_string(&sse));
                }
            }
            None => {
                // Hand the client an event id to resume from. Without one, a
                // client polling this stream would reconnect with no
                // `Last-Event-ID` and never be given what was stored since.
                let anchor = match session.last_event_id.clone() {
                    Some(id) => Some(id),
                    None => self.store_event(&session.id, "").await,
                };
                if let Some(anchor) = anchor {
                    body.push_str(&SseEncoder::encode_string(&SseEvent::with_id(anchor, "")));
                }
            }
        }

        Reply::sse(body).with_header("Mcp-Session-Id", session.id.into_string())
    }

    /// Handle DELETE: terminate the session.
    async fn handle_delete(&self, headers: &HeaderMap) -> Reply {
        if !self.config.enable_sessions {
            return Reply::method_not_allowed("POST, OPTIONS");
        }
        match self.resolve_session(headers).await {
            Ok(session) => {
                self.end_session(&session.id).await;
                Reply::empty(204)
            }
            Err(reply) => reply,
        }
    }

    /// Store an event for replay support.
    ///
    /// Call this when sending server-initiated messages to enable
    /// client reconnection with `Last-Event-ID`.
    pub async fn store_event(&self, session_id: &SessionId, data: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        let seq = {
            let mut seq = self.event_sequence.borrow_mut();
            *seq += 1;
            *seq
        };
        #[cfg(not(target_arch = "wasm32"))]
        let seq = self
            .event_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        let event_id = turbomcp_transport_streamable::sse::generate_event_id(seq);

        let event = StoredEvent::new_with_timestamp(event_id.clone(), data, current_timestamp_ms());
        self.session_store
            .store_event(session_id, event)
            .await
            .ok()?;

        // Remember the newest id so a fresh GET can anchor the client there.
        if let Ok(Some(mut session)) = self.session_store.get(session_id).await {
            session.last_event_id = Some(event_id.clone());
            session.event_count += 1;
            let _ = self.session_store.update(&session).await;
        }
        Some(event_id)
    }
}

/// Whether the request's `Accept` allows `mime`. A missing header allows
/// anything; `*/*` and `type/*` count.
fn accepts(headers: &HeaderMap, mime: &str) -> bool {
    let Some(accept) = headers.get("accept") else {
        return true;
    };
    let family = mime.split('/').next().unwrap_or("");
    accept.split(',').any(|entry| {
        let entry = entry.split(';').next().unwrap_or("").trim();
        entry == "*/*"
            || entry.eq_ignore_ascii_case(mime)
            || entry
                .strip_suffix("/*")
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(family))
    })
}

/// Extension trait to serve any handler over Streamable HTTP.
pub trait StreamableExt: McpHandler {
    /// Convert this handler into a streamable HTTP handler.
    fn into_streamable(self) -> StreamableHandler<MemorySessionStore, Self>;
}

impl<H: McpHandler> StreamableExt for H {
    fn into_streamable(self) -> StreamableHandler<MemorySessionStore, Self> {
        StreamableHandler::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use turbomcp_transport_streamable::SessionState;

    fn server() -> McpServer {
        McpServer::builder("streamable-test", "1.0.0")
            .tool_raw(
                "echo",
                "Echo",
                |args: Value| async move { args.to_string() },
            )
            .build()
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map: HeaderMap = [
            ("content-type", "application/json"),
            ("accept", "application/json, text/event-stream"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        for (k, v) in pairs {
            map.insert(k.to_string(), v.to_string());
        }
        map
    }

    const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#;

    async fn open(handler: &StreamableHandler) -> String {
        let reply = handler.process("POST", &headers(&[]), Some(INIT)).await;
        assert_eq!(reply.status, 200);
        reply
            .headers
            .iter()
            .find(|(name, _)| *name == "Mcp-Session-Id")
            .map(|(_, id)| id.clone())
            .expect("initialize issues a session")
    }

    fn body(reply: &Reply) -> Value {
        serde_json::from_str(&reply.body).unwrap()
    }

    #[tokio::test]
    async fn test_memory_session_store() {
        let store = MemorySessionStore::new();

        // Create a session
        let id = store.create().await.unwrap();
        assert!(id.as_str().starts_with("mcp-"));

        // Get the session
        let session = store.get(&id).await.unwrap().unwrap();
        assert_eq!(session.state, SessionState::Pending);

        // Update the session
        let mut session = session;
        session.activate();
        store.update(&session).await.unwrap();

        let updated = store.get(&id).await.unwrap().unwrap();
        assert_eq!(updated.state, SessionState::Active);

        // Store and replay events
        let event1 = StoredEvent::new("evt-1", "data1");
        let event2 = StoredEvent::new("evt-2", "data2");
        store.store_event(&id, event1).await.unwrap();
        store.store_event(&id, event2).await.unwrap();

        let replayed = store.replay_from(&id, "evt-1").await.unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].id, "evt-2");

        // Destroy the session
        store.destroy(&id).await.unwrap();
        assert!(store.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unknown_last_event_id_replays_nothing() {
        let store = MemorySessionStore::new();
        let id = store.create().await.unwrap();
        store
            .store_event(&id, StoredEvent::new("evt-1", "data1"))
            .await
            .unwrap();
        assert!(store.replay_from(&id, "forged").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_store_is_bounded() {
        let store = MemorySessionStore::new()
            .with_max_sessions(2)
            .with_max_events_per_session(2);

        let first = store.create().await.unwrap();
        let _second = store.create().await.unwrap();
        let third = store.create().await.unwrap();
        assert_eq!(store.lock().sessions.len(), 2);
        assert!(store.get(&third).await.unwrap().is_some());
        let _ = first;

        for n in 0..5 {
            store
                .store_event(&third, StoredEvent::new(format!("evt-{n}"), "x"))
                .await
                .unwrap();
        }
        let kept: Vec<_> = store.lock().events[third.as_str()]
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(kept, ["evt-3", "evt-4"]);
    }

    #[tokio::test]
    async fn session_is_issued_only_for_a_successful_initialize() {
        let handler = StreamableHandler::new(server());

        let bad = handler
            .process(
                "POST",
                &headers(&[]),
                Some(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#),
            )
            .await;
        assert_eq!(body(&bad)["error"]["code"], -32602);
        assert!(
            bad.headers
                .iter()
                .all(|(name, _)| *name != "Mcp-Session-Id")
        );
        assert!(handler.session_store.lock().sessions.is_empty());

        let id = open(&handler).await;
        let session = handler
            .session_store
            .get(&SessionId::from_string(id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.protocol_version.as_deref(), Some("2025-06-18"));
        assert!(session.is_active());
    }

    #[tokio::test]
    async fn requests_after_initialize_need_a_live_session() {
        let handler = StreamableHandler::new(server());
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;

        let missing = handler.process("POST", &headers(&[]), Some(list)).await;
        assert_eq!(missing.status, 400);

        let unknown = handler
            .process(
                "POST",
                &headers(&[("mcp-session-id", "mcp-nope")]),
                Some(list),
            )
            .await;
        assert_eq!(unknown.status, 404);

        let id = open(&handler).await;
        let ok = handler
            .process("POST", &headers(&[("mcp-session-id", &id)]), Some(list))
            .await;
        assert_eq!(ok.status, 200);

        // Terminated sessions are 404, not 410: 404 is what tells a client to
        // start over with a new initialize.
        let deleted = handler
            .process("DELETE", &headers(&[("mcp-session-id", &id)]), None)
            .await;
        assert_eq!(deleted, Reply::empty(204));
        let after = handler
            .process("POST", &headers(&[("mcp-session-id", &id)]), Some(list))
            .await;
        assert_eq!(after.status, 404);
    }

    #[tokio::test]
    async fn ping_is_answered_without_a_session() {
        let handler = StreamableHandler::new(server());
        let reply = handler
            .process(
                "POST",
                &headers(&[]),
                Some(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#),
            )
            .await;
        assert_eq!(reply.status, 200);
    }

    #[tokio::test]
    async fn protocol_version_header_must_match_the_session() {
        let handler = StreamableHandler::new(server());
        let id = open(&handler).await;
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;

        let mismatched = handler
            .process(
                "POST",
                &headers(&[
                    ("mcp-session-id", &id),
                    (PROTOCOL_VERSION_HEADER, "2025-11-25"),
                ]),
                Some(list),
            )
            .await;
        assert_eq!(mismatched.status, 400);

        let unsupported = handler
            .process(
                "POST",
                &headers(&[
                    ("mcp-session-id", &id),
                    (PROTOCOL_VERSION_HEADER, "1999-01-01"),
                ]),
                Some(list),
            )
            .await;
        assert_eq!(unsupported.status, 400);

        let matching = handler
            .process(
                "POST",
                &headers(&[
                    ("mcp-session-id", &id),
                    (PROTOCOL_VERSION_HEADER, "2025-06-18"),
                ]),
                Some(list),
            )
            .await;
        assert_eq!(matching.status, 200);
    }

    #[tokio::test]
    async fn session_version_steps_responses_down() {
        let handler = StreamableHandler::new(server());
        let id = open(&handler).await;
        // `tasks/list` does not exist in 2025-06-18, the version this session
        // negotiated, even without a header restating it.
        let reply = handler
            .process(
                "POST",
                &headers(&[("mcp-session-id", &id)]),
                Some(r#"{"jsonrpc":"2.0","id":2,"method":"tasks/list"}"#),
            )
            .await;
        assert_eq!(body(&reply)["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn notifications_and_client_responses_get_an_empty_202() {
        let handler = StreamableHandler::new(server());
        let id = open(&handler).await;
        for message in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
        ] {
            let reply = handler
                .process("POST", &headers(&[("mcp-session-id", &id)]), Some(message))
                .await;
            assert_eq!(reply, Reply::empty(202), "{message}");
        }
    }

    #[tokio::test]
    async fn idle_sessions_expire() {
        let handler = StreamableHandler::new(server())
            .with_config(StreamableConfig::default().with_idle_timeout_ms(1_000));
        let id = open(&handler).await;
        {
            let mut state = handler.session_store.lock();
            let session = state.sessions.get_mut(&id).unwrap();
            session.last_activity = session.last_activity.saturating_sub(5_000);
        }
        let reply = handler
            .process(
                "POST",
                &headers(&[("mcp-session-id", &id)]),
                Some(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#),
            )
            .await;
        assert_eq!(reply.status, 404);
        assert!(handler.session_store.lock().sessions.is_empty());
    }

    #[tokio::test]
    async fn get_replays_only_what_follows_the_last_event_id() {
        let handler = StreamableHandler::new(server());
        let id = open(&handler).await;
        let session_id = SessionId::from_string(id.clone());

        // A fresh GET anchors the client.
        let first = handler
            .process("GET", &headers(&[("mcp-session-id", &id)]), None)
            .await;
        assert_eq!(first.content_type, Some("text/event-stream"));
        assert!(first.body.contains("retry: "));
        let anchor = first
            .body
            .lines()
            .find_map(|line| line.strip_prefix("id: "))
            .expect("anchor id")
            .to_string();

        let stored = handler
            .store_event(
                &session_id,
                r#"{"jsonrpc":"2.0","method":"notifications/message"}"#,
            )
            .await
            .unwrap();

        let resumed = handler
            .process(
                "GET",
                &headers(&[("mcp-session-id", &id), ("last-event-id", &anchor)]),
                None,
            )
            .await;
        assert!(resumed.body.contains(&format!("id: {stored}")));

        let forged = handler
            .process(
                "GET",
                &headers(&[("mcp-session-id", &id), ("last-event-id", "forged")]),
                None,
            )
            .await;
        assert!(!forged.body.contains("notifications/message"));
    }

    #[test]
    fn precheck_rejects_before_the_body_is_read() {
        let handler = StreamableHandler::new(server());

        let put = handler.precheck("PUT", &headers(&[])).unwrap_err();
        assert_eq!(put.status, 405);
        assert!(
            put.headers
                .contains(&("Allow", ALLOWED_METHODS.to_string()))
        );

        let evil = handler
            .precheck("POST", &headers(&[("origin", "https://evil.com")]))
            .unwrap_err();
        assert_eq!(evil.status, 403);
        assert!(
            handler
                .precheck("POST", &headers(&[("origin", "http://localhost:5173")]))
                .is_ok()
        );

        let mut text = headers(&[]);
        text.insert("content-type".into(), "text/plain".into());
        assert_eq!(handler.precheck("POST", &text).unwrap_err().status, 415);

        let big = headers(&[("content-length", "999999999")]);
        assert_eq!(handler.precheck("POST", &big).unwrap_err().status, 413);

        let strict = StreamableHandler::new(server())
            .with_config(StreamableConfig::default().with_require_origin(true));
        assert_eq!(
            strict.precheck("POST", &headers(&[])).unwrap_err().status,
            403
        );

        let open = StreamableHandler::new(server()).allow_any_origin(true);
        assert!(
            open.precheck("POST", &headers(&[("origin", "https://evil.com")]))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn accept_must_allow_the_answer() {
        let handler = StreamableHandler::new(server());
        let reply = handler
            .process("POST", &headers(&[("accept", "text/html")]), Some(INIT))
            .await;
        assert_eq!(reply.status, 406);

        assert!(accepts(&headers(&[("accept", "*/*")]), "text/event-stream"));
        assert!(accepts(
            &headers(&[("accept", "application/*")]),
            "application/json"
        ));
        assert!(!accepts(
            &headers(&[("accept", "application/json")]),
            "text/event-stream"
        ));
    }

    #[tokio::test]
    async fn wrappers_are_served_too() {
        use crate::wasm_server::{MiddlewareStack, VisibilityLayer};
        let wrapped = VisibilityLayer::new(MiddlewareStack::new(server())).into_streamable();
        let reply = wrapped.process("POST", &headers(&[]), Some(INIT)).await;
        assert_eq!(body(&reply)["result"]["protocolVersion"], "2025-06-18");
    }
}
