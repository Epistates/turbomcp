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
//! # Version-Aware Routing
//!
//! Per-session version-aware routing is active. After a successful `initialize`
//! handshake, the negotiated [`ProtocolVersion`] is stored in [`SessionManager`]
//! keyed by `Mcp-Session-Id`. All subsequent requests for that session are
//! dispatched through [`router::route_request_versioned`], ensuring correct
//! adapter filtering and method availability for the negotiated spec version.

use std::collections::{HashMap, HashSet};
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
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
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
use crate::context::{McpSession, RequestContext, SessionFuture};
use crate::router::{self, JsonRpcIncoming, JsonRpcOutgoing};

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

type PendingServerResponse = oneshot::Sender<McpResult<serde_json::Value>>;
type PendingServerRequests = Arc<Mutex<HashMap<String, PendingServerResponse>>>;

/// Per-session data tracked by SessionManager.
///
/// The MCP 2025-11-25 spec (§Multiple Connections) says a server "MUST send
/// each of its JSON-RPC messages on only one of the connected streams; that
/// is, it MUST NOT broadcast the same message across multiple streams."
/// We therefore track subscribers as a list of mpsc senders and route each
/// outbound message to exactly one of them, dropping dead senders as we go.
#[derive(Debug)]
struct SessionData {
    /// Ordered list of active SSE subscribers (newest last).
    subscribers: Vec<mpsc::UnboundedSender<String>>,
    /// Negotiated protocol version (set after successful initialize).
    protocol_version: Option<ProtocolVersion>,
    /// Client capabilities captured from the successful initialize request.
    client_capabilities: Option<ClientCapabilities>,
    /// Request IDs already used by the client within this session.
    seen_request_ids: HashSet<String>,
    /// Pending responses for server-initiated requests sent over SSE.
    pending_server_requests: PendingServerRequests,
    /// Monotonic server request counter. IDs are rendered as `s-{n}`.
    next_server_request_id: u64,
}

/// Session manager for SSE connections.
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
    /// Create a new session manager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session and return the session ID.
    pub async fn create_session(
        &self,
        initialize_request_id: Option<&serde_json::Value>,
    ) -> String {
        let session_id = Uuid::new_v4().to_string();
        let mut seen_request_ids = HashSet::new();
        if let Some(request_id) = initialize_request_id.and_then(super::request_id_key) {
            seen_request_ids.insert(request_id);
        }

        self.sessions.write().await.insert(
            session_id.clone(),
            SessionData {
                subscribers: Vec::new(),
                protocol_version: None,
                client_capabilities: None,
                seen_request_ids,
                pending_server_requests: Arc::new(Mutex::new(HashMap::new())),
                next_server_request_id: 1,
            },
        );

        tracing::debug!("Created SSE session: {}", session_id);
        session_id
    }

    /// Remove a session.
    pub async fn remove_session(&self, session_id: &str) -> bool {
        let removed = self.sessions.write().await.remove(session_id).is_some();
        if removed {
            tracing::debug!("Removed session: {}", session_id);
        }
        removed
    }

    /// Subscribe to an existing session's SSE stream.
    ///
    /// Each subscribe returns a dedicated [`mpsc::UnboundedReceiver`] that
    /// only receives messages routed to this subscriber — never broadcasts.
    pub async fn subscribe_session(
        &self,
        session_id: &str,
    ) -> Option<mpsc::UnboundedReceiver<String>> {
        let mut sessions = self.sessions.write().await;
        let data = sessions.get_mut(session_id)?;
        let (tx, rx) = mpsc::unbounded_channel();
        data.subscribers.push(tx);
        Some(rx)
    }

    /// Check whether a session exists.
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    /// Send a message to one subscriber for the given session.
    ///
    /// Per the MCP Multiple Connections rule, this routes the message to
    /// exactly one of the session's currently connected streams (the most
    /// recently subscribed live one), dropping any closed senders along the
    /// way. Returns `true` if the message was delivered.
    ///
    /// Public for `benches/sse_throughput.rs`; treat as crate-internal.
    #[doc(hidden)]
    pub async fn send_to_session(&self, session_id: &str, message: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        let Some(data) = sessions.get_mut(session_id) else {
            return false;
        };
        // Drain dead senders from the newest end forward until we find a
        // live one that accepts the message. This gives new SSE connections
        // priority over stale ones without closing streams that are idle.
        while let Some(tx) = data.subscribers.last() {
            if tx.is_closed() {
                data.subscribers.pop();
                continue;
            }
            if tx.send(message.to_string()).is_ok() {
                return true;
            }
            // Send only fails here if the receiver was dropped between the
            // is_closed check and send; pop and retry.
            data.subscribers.pop();
        }
        false
    }

    /// Broadcast a message to one subscriber per session.
    ///
    /// Iterates every session and routes to a single live subscriber
    /// following the same per-session rule as [`Self::send_to_session`].
    ///
    /// Reserved for server-initiated push (not yet wired). Public for
    /// `benches/sse_throughput.rs`; treat as crate-internal.
    #[doc(hidden)]
    pub async fn broadcast(&self, message: &str) {
        let mut sessions = self.sessions.write().await;
        for (session_id, data) in sessions.iter_mut() {
            let mut delivered = false;
            while let Some(tx) = data.subscribers.last() {
                if tx.is_closed() {
                    data.subscribers.pop();
                    continue;
                }
                if tx.send(message.to_string()).is_ok() {
                    delivered = true;
                    break;
                }
                data.subscribers.pop();
            }
            if !delivered {
                tracing::warn!("No live subscriber for session {}", session_id);
            }
        }
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

    /// Register a request ID for an existing session.
    pub(crate) async fn register_request_id(
        &self,
        session_id: &str,
        request_id: Option<&serde_json::Value>,
    ) -> bool {
        let Some(request_id) = request_id.and_then(super::request_id_key) else {
            return true;
        };

        self.sessions
            .write()
            .await
            .get_mut(session_id)
            .is_some_and(|data| data.seen_request_ids.insert(request_id))
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
}

/// Bidirectional HTTP/SSE session handle used by request handlers.
#[derive(Debug, Clone)]
struct HttpSessionHandle {
    session_id: String,
    session_manager: SessionManager,
    request_timeout: Duration,
}

impl HttpSessionHandle {
    fn new(session_id: impl Into<String>, session_manager: SessionManager) -> Self {
        Self {
            session_id: session_id.into(),
            session_manager,
            request_timeout: Duration::from_secs(SERVER_REQUEST_TIMEOUT_SECS),
        }
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

            if !self
                .session_manager
                .send_to_session(&self.session_id, &payload)
                .await
            {
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

            if self
                .session_manager
                .send_to_session(&self.session_id, &payload)
                .await
            {
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
) -> router::JsonRpcOutgoing {
    let ctx = http_request_context(session_manager, session_id, request.id.as_ref());

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

fn http_request_context(
    session_manager: &SessionManager,
    session_id: Option<&str>,
    request_id: Option<&serde_json::Value>,
) -> RequestContext {
    let mut ctx = RequestContext::http();

    if let Some(request_id) = request_id.and_then(super::request_id_key) {
        ctx = ctx.with_request_id(request_id);
    }

    if let Some(session_id) = session_id {
        let session = Arc::new(HttpSessionHandle::new(
            session_id.to_string(),
            session_manager.clone(),
        )) as Arc<dyn McpSession>;
        ctx = ctx
            .with_session_id(session_id.to_string())
            .with_session(session);
    }

    ctx
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

/// Frame one payload as an SSE event (`id:` / `event:` / `data:` lines).
///
/// Public for `benches/sse_throughput.rs`; treat as crate-internal.
#[doc(hidden)]
pub fn sse_event_bytes(id: &str, event_type: Option<&str>, data: &str) -> Bytes {
    let mut event = String::new();
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
            event.push_str(line.strip_suffix('\r').unwrap_or(line));
            event.push('\n');
        }
    }

    event.push('\n');
    Bytes::from(event)
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
        return Ok(None);
    }

    // MCP 2025-11-25 lifecycle permits ping before the server has responded
    // to initialize. With no session yet, route it as a sessionless request.
    if method == "ping" && session_id.is_none() {
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

    if let Some(session_id) = session_id.as_deref()
        && !state
            .session_manager
            .register_request_id(session_id, request.id.as_ref())
            .await
    {
        return json_response(
            StatusCode::OK,
            JsonRpcOutgoing::error(
                request.id.clone(),
                McpError::invalid_request("Request ID already used in this session"),
            ),
        );
    }

    let initialize_request_id = request.id.clone();
    let response = route_with_version_tracking(
        &state.handler,
        request,
        &state.session_manager,
        state.config.as_ref(),
        session_id.as_deref(),
    )
    .await;

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
    let Some(mut rx) = state.session_manager.subscribe_session(&session_id).await else {
        return empty_response(StatusCode::NOT_FOUND);
    };

    // Create the SSE stream. Each GET subscription gets its own `stream_id` so
    // Event IDs include a stream component so concurrent streams on the same
    // session produce distinguishable IDs. Format: `{session_id}-{stream_id}-{seq}`.
    // A future replay buffer can use the (stream_id, seq) tuple to resume from
    // an arbitrary Last-Event-ID.
    let stream_id = Uuid::new_v4().simple().to_string();
    let primer_id = format!("{}-{}-0", session_id, stream_id);
    let session_id_for_events = session_id.clone();
    let stream_id_for_events = stream_id;
    let stream = async_stream::stream! {
        // This is the GET listening stream. Resumption in MCP is always via GET
        // + `Last-Event-ID`, so the spec's *required* primer event applies to
        // POST-initiated SSE streams (§Sending Messages, item 6) — and this
        // server answers POST with a single `application/json` object rather
        // than a stream, so that requirement never engages here. We still emit a
        // primer on the GET stream: §Resumability permits attaching event IDs,
        // and doing so hands the client an immediate `Last-Event-ID` anchor to
        // resume from. Send the `: connected` comment first so older RMCP/Codex
        // clients that misparse `data:\n\n` as a JSON-RPC payload are
        // unaffected, then the primer event (event ID + empty data field).
        yield Ok::<_, std::convert::Infallible>(Bytes::from_static(b": connected\n\n"));
        yield Ok::<_, std::convert::Infallible>(sse_event_bytes(&primer_id, None, ""));

        // Drain messages routed to this specific subscriber. Per spec we
        // only see messages that the server explicitly chose to send to
        // this stream; other concurrent streams on the same session have
        // their own receivers. The primer above consumed seq 0, so real
        // message IDs start at 1 — §Resumability requires event IDs to be
        // globally unique within the session, and reusing `-0` here would
        // make the primer and the first message indistinguishable on replay.
        let mut seq: u64 = 1;
        loop {
            match tokio::time::timeout(Duration::from_secs(SSE_KEEP_ALIVE_SECS), rx.recv()).await {
                Ok(Some(message)) => {
                    let event_id = format!(
                        "{}-{}-{}",
                        session_id_for_events, stream_id_for_events, seq
                    );
                    seq = seq.saturating_add(1);
                    yield Ok::<_, std::convert::Infallible>(sse_event_bytes(
                        &event_id,
                        Some("message"),
                        &message,
                    ));
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

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .expect("SSE response builder should be valid");
    response
        .headers_mut()
        .insert("mcp-session-id", session_header_value(&session_id));
    response
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

        let first_got = matches!(first, Ok(Some(ref s)) if s == "hello");
        let second_got = matches!(second, Ok(Some(ref s)) if s == "hello");

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
