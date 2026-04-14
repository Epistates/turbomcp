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
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tokio::sync::{RwLock, broadcast};
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::jsonrpc::JsonRpcResponse as CoreJsonRpcResponse;
use turbomcp_core::types::core::ProtocolVersion;
use turbomcp_transport::security::{
    OriginConfig, SecurityHeaders, extract_client_ip, validate_origin,
};
use uuid::Uuid;

use crate::config::{RateLimiter, ServerConfig};
use crate::context::RequestContext;
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

/// Per-session data tracked by SessionManager.
#[derive(Debug, Clone)]
struct SessionData {
    /// Broadcast channel for SSE push.
    tx: broadcast::Sender<String>,
    /// Negotiated protocol version (set after successful initialize).
    protocol_version: Option<ProtocolVersion>,
    /// Request IDs already used by the client within this session.
    seen_request_ids: HashSet<String>,
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
        let (tx, _) = broadcast::channel(100);
        let mut seen_request_ids = HashSet::new();
        if let Some(request_id) = initialize_request_id.and_then(super::request_id_key) {
            seen_request_ids.insert(request_id);
        }

        self.sessions.write().await.insert(
            session_id.clone(),
            SessionData {
                tx,
                protocol_version: None,
                seen_request_ids,
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
    pub async fn subscribe_session(&self, session_id: &str) -> Option<broadcast::Receiver<String>> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|data| data.tx.subscribe())
    }

    /// Check whether a session exists.
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    /// Send a message to a specific session.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn send_to_session(&self, session_id: &str, message: &str) -> bool {
        if let Some(data) = self.sessions.read().await.get(session_id) {
            data.tx.send(message.to_string()).is_ok()
        } else {
            false
        }
    }

    /// Broadcast a message to all sessions.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn broadcast(&self, message: &str) {
        let sessions = self.sessions.read().await;
        for (session_id, data) in sessions.iter() {
            if data.tx.send(message.to_string()).is_err() {
                tracing::warn!("Failed to send to session {}", session_id);
            }
        }
    }

    /// Get the number of active sessions.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Store the negotiated protocol version for a session.
    pub(crate) async fn set_protocol_version(&self, session_id: &str, version: ProtocolVersion) {
        if let Some(data) = self.sessions.write().await.get_mut(session_id) {
            data.protocol_version = Some(version);
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
    .await
    .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
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
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
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
    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();

    if request.method == "initialize" {
        let response = router::route_request_with_config(handler, request, &core_ctx, config).await;

        // If successful and we have a session, extract and store the negotiated version.
        if let (Some(sid), Some(result)) = (session_id, response.result.as_ref())
            && let Some(version_str) = result.get("protocolVersion").and_then(|v| v.as_str())
        {
            let version = ProtocolVersion::from(version_str);
            session_manager.set_protocol_version(sid, version).await;
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
        return router::route_request_versioned(handler, request, &core_ctx, &version).await;
    }

    // Pre-initialize or sessionless: route with config for proper validation.
    router::route_request_with_config(handler, request, &core_ctx, config).await
}

fn parse_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
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

fn extract_request_ip(headers: &HeaderMap, extensions: &axum::http::Extensions) -> Option<IpAddr> {
    let security_headers = to_security_headers(headers);
    extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0.ip())
        .or_else(|| extract_client_ip(&security_headers))
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

fn validate_protocol_header(
    headers: &HeaderMap,
    config: Option<&ServerConfig>,
    expected: Option<&ProtocolVersion>,
) -> Result<(), StatusCode> {
    let Some(raw) = headers.get("mcp-protocol-version") else {
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

/// Axum handler for JSON-RPC requests (simple mode).
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    request: axum::http::Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let client_ip = extract_request_ip(&headers, &parts.extensions);
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

    let payload = match to_bytes(body, MAX_BODY_SIZE).await {
        Ok(body) => match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(payload) => payload,
            Err(_) => return empty_response(StatusCode::BAD_REQUEST),
        },
        Err(_) => return empty_response(StatusCode::BAD_REQUEST),
    };

    let request = match serde_json::from_value::<JsonRpcIncoming>(payload.clone()) {
        Ok(request) => request,
        Err(_) => {
            if serde_json::from_value::<CoreJsonRpcResponse>(payload).is_ok() {
                return empty_response(StatusCode::ACCEPTED);
            }
            return empty_response(StatusCode::BAD_REQUEST);
        }
    };
    let is_initialize = request.method == "initialize";
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
            .set_protocol_version(&session_id, ProtocolVersion::from(version_str))
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
    let client_ip = extract_request_ip(&headers, &parts.extensions);
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

    // Create the SSE stream
    let stream = async_stream::stream! {
        // Listen for messages from the broadcast channel
        loop {
            match rx.recv().await {
                Ok(message) => {
                    yield Ok::<_, std::convert::Infallible>(
                        Event::default().event("message").data(message),
                    );
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE client lagged, missed {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("SSE broadcast channel closed");
                    break;
                }
            }
        }
    };

    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(SSE_KEEP_ALIVE_SECS))
                .text("keep-alive"),
        )
        .into_response();
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
    let client_ip = extract_request_ip(&headers, &parts.extensions);
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
    // HTTP tests are in /tests/ as they require network access
}
