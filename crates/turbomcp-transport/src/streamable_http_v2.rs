//! MCP 2025-06-18 Compliant Streamable HTTP Transport - Standard Implementation
//!
//! This transport provides **strict MCP 2025-06-18 specification compliance** with:
//! - ✅ Single MCP endpoint supporting GET, POST, and DELETE
//! - ✅ SSE streaming responses from POST requests
//! - ✅ Proper "endpoint" event for dynamic URL discovery
//! - ✅ Message replay for Last-Event-ID resumability
//! - ✅ Session management with Mcp-Session-Id headers
//! - ✅ Protocol version negotiation with MCP-Protocol-Version
//! - ✅ Industrial-grade security (Origin validation, rate limiting, IP binding)
//! - ✅ 202 Accepted for all notifications and responses
//!
//! **Architecture:**
//! - Unified endpoint pattern (no separate /sse and /rpc paths)
//! - Message buffer for replay support
//! - Accept header negotiation (JSON vs SSE)
//! - Multiple concurrent SSE streams per session
//! - Secure session management with IP binding

use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::security::{
    SecurityConfigBuilder, SecurityHeaders, SecurityValidator, SessionSecurityConfig,
    SessionSecurityManager,
};

/// Maximum events to buffer for replay (per session)
const MAX_REPLAY_BUFFER: usize = 1000;

/// Configuration for streamable HTTP transport
#[derive(Clone, Debug)]
pub struct StreamableHttpConfig {
    /// Bind address (default: 127.0.0.1:8080 for security)
    pub bind_addr: String,

    /// Base path for MCP endpoint (default: "/mcp")
    pub endpoint_path: String,

    /// SSE keep-alive interval
    pub keep_alive: Duration,

    /// Message replay buffer size
    pub replay_buffer_size: usize,

    /// Security validator
    pub security_validator: Arc<SecurityValidator>,

    /// Session manager
    pub session_manager: Arc<SessionSecurityManager>,
}

impl Default for StreamableHttpConfig {
    fn default() -> Self {
        StreamableHttpConfigBuilder::new().build()
    }
}

/// Builder for StreamableHttpConfig with ergonomic configuration
///
/// # Examples
///
/// ```rust
/// use turbomcp_transport::streamable_http_v2::StreamableHttpConfigBuilder;
/// use std::time::Duration;
///
/// // Custom rate limits for benchmarking
/// let config = StreamableHttpConfigBuilder::new()
///     .with_bind_address("127.0.0.1:3000")
///     .with_rate_limit(100_000, Duration::from_secs(60))
///     .build();
///
/// // Production configuration
/// let config = StreamableHttpConfigBuilder::new()
///     .with_bind_address("0.0.0.0:8080")
///     .with_endpoint_path("/api/mcp")
///     .with_rate_limit(1000, Duration::from_secs(60))
///     .allow_any_origin(true)
///     .require_authentication(true)
///     .build();
///
/// // Development (no rate limit)
/// let config = StreamableHttpConfigBuilder::new()
///     .without_rate_limit()
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct StreamableHttpConfigBuilder {
    bind_addr: String,
    endpoint_path: String,
    keep_alive: Duration,
    replay_buffer_size: usize,

    // Security configuration
    allow_localhost: bool,
    allow_any_origin: bool,
    require_authentication: bool,
    rate_limit: Option<(u32, Duration)>,
}

impl Default for StreamableHttpConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamableHttpConfigBuilder {
    /// Create a new builder with sensible defaults
    pub fn new() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            endpoint_path: "/mcp".to_string(),
            keep_alive: Duration::from_secs(30),
            replay_buffer_size: MAX_REPLAY_BUFFER,
            allow_localhost: true,
            allow_any_origin: false,
            require_authentication: false,
            rate_limit: Some((100, Duration::from_secs(60))), // Default: 100 req/min
        }
    }

    /// Set the bind address (default: "127.0.0.1:8080")
    pub fn with_bind_address(mut self, addr: impl Into<String>) -> Self {
        self.bind_addr = addr.into();
        self
    }

    /// Set the endpoint path (default: "/mcp")
    pub fn with_endpoint_path(mut self, path: impl Into<String>) -> Self {
        self.endpoint_path = path.into();
        self
    }

    /// Set the SSE keep-alive interval (default: 30 seconds)
    pub fn with_keep_alive(mut self, duration: Duration) -> Self {
        self.keep_alive = duration;
        self
    }

    /// Set the replay buffer size (default: 1000 events)
    pub fn with_replay_buffer_size(mut self, size: usize) -> Self {
        self.replay_buffer_size = size;
        self
    }

    /// Configure rate limiting (requests per time window)
    ///
    /// # Examples
    /// ```rust
    /// use turbomcp_transport::streamable_http_v2::StreamableHttpConfigBuilder;
    /// use std::time::Duration;
    ///
    /// // 1000 requests per minute
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_rate_limit(1000, Duration::from_secs(60))
    ///     .build();
    ///
    /// // 100,000 requests per minute (benchmarking)
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_rate_limit(100_000, Duration::from_secs(60))
    ///     .build();
    /// ```
    pub fn with_rate_limit(mut self, requests: u32, window: Duration) -> Self {
        self.rate_limit = Some((requests, window));
        self
    }

    /// Disable rate limiting entirely (useful for development/testing)
    ///
    /// # Security Warning
    /// Only use this in trusted environments. Production deployments should
    /// always have rate limiting enabled.
    pub fn without_rate_limit(mut self) -> Self {
        self.rate_limit = None;
        self
    }

    /// Allow localhost connections (default: true)
    pub fn allow_localhost(mut self, allow: bool) -> Self {
        self.allow_localhost = allow;
        self
    }

    /// Allow any origin for CORS (default: false)
    ///
    /// # Security Warning
    /// Only enable in development. Production should specify exact origins.
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.allow_any_origin = allow;
        self
    }

    /// Require authentication (default: false)
    pub fn require_authentication(mut self, require: bool) -> Self {
        self.require_authentication = require;
        self
    }

    /// Build the configuration
    pub fn build(self) -> StreamableHttpConfig {
        let mut security_builder = SecurityConfigBuilder::new()
            .allow_localhost(self.allow_localhost)
            .allow_any_origin(self.allow_any_origin)
            .require_authentication(self.require_authentication);

        // Add rate limit if configured
        if let Some((requests, window)) = self.rate_limit {
            security_builder = security_builder.with_rate_limit(requests as usize, window);
        }

        let security_validator = Arc::new(security_builder.build());
        let session_manager =
            Arc::new(SessionSecurityManager::new(SessionSecurityConfig::default()));

        StreamableHttpConfig {
            bind_addr: self.bind_addr,
            endpoint_path: self.endpoint_path,
            keep_alive: self.keep_alive,
            replay_buffer_size: self.replay_buffer_size,
            security_validator,
            session_manager,
        }
    }
}

/// SSE event with metadata for replay
#[derive(Clone, Debug)]
struct StoredEvent {
    id: String,
    event_type: String,
    data: String,
}

/// Session state with message replay buffer
struct Session {
    event_buffer: VecDeque<StoredEvent>,
    sse_senders: Vec<mpsc::UnboundedSender<StoredEvent>>,
}

impl Session {
    fn new(buffer_size: usize) -> Self {
        Self {
            event_buffer: VecDeque::with_capacity(buffer_size),
            sse_senders: Vec::new(),
        }
    }

    /// Add event to buffer and broadcast to all connected streams
    fn broadcast_event(&mut self, event: StoredEvent) {
        // Add to replay buffer
        if self.event_buffer.len() >= self.event_buffer.capacity() {
            self.event_buffer.pop_front();
        }
        self.event_buffer.push_back(event.clone());

        // Broadcast to all active SSE streams
        self.sse_senders
            .retain(|sender| sender.send(event.clone()).is_ok());
    }

    /// Get events after a specific event ID for replay
    fn replay_from(&self, last_event_id: &str) -> Vec<StoredEvent> {
        let mut found = false;
        self.event_buffer
            .iter()
            .filter(|event| {
                if found {
                    true
                } else if event.id == last_event_id {
                    found = true;
                    false
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }
}

/// Shared application state
struct AppState<H: turbomcp_protocol::JsonRpcHandler> {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    security_validator: Arc<SecurityValidator>,
    session_manager: Arc<SessionSecurityManager>,
    config: StreamableHttpConfig,
    handler: Arc<H>,
}

impl<H: turbomcp_protocol::JsonRpcHandler> Clone for AppState<H> {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone(),
            security_validator: self.security_validator.clone(),
            session_manager: self.session_manager.clone(),
            config: self.config.clone(),
            handler: self.handler.clone(),
        }
    }
}

/// Create MCP-compliant router with single unified endpoint
///
/// # Type Parameters
///
/// * `H` - JSON-RPC handler implementation (typically macro-generated)
///
/// # Arguments
///
/// * `config` - Transport configuration
/// * `handler` - JSON-RPC request handler
///
/// # Returns
///
/// Axum router configured with GET/POST/DELETE handlers for full MCP 2025-06-18 compliance
pub fn create_router<H: turbomcp_protocol::JsonRpcHandler>(
    config: StreamableHttpConfig,
    handler: Arc<H>,
) -> Router {
    let state = AppState {
        sessions: Arc::new(RwLock::new(HashMap::new())),
        security_validator: config.security_validator.clone(),
        session_manager: config.session_manager.clone(),
        config: config.clone(),
        handler,
    };

    Router::new()
        // Single MCP endpoint - handles GET, POST, DELETE
        .route(
            &config.endpoint_path,
            get(mcp_get_handler::<H>)
                .post(mcp_post_handler::<H>)
                .delete(mcp_delete_handler::<H>),
        )
        .with_state(state)
}

/// GET handler - Opens SSE stream for server-initiated communication
///
/// Per MCP 2025-06-18:
/// - Returns text/event-stream
/// - First event MUST be type="endpoint" with message endpoint URL
/// - Supports Last-Event-ID for resumability
/// - Can send server requests and notifications
async fn mcp_get_handler<H: turbomcp_protocol::JsonRpcHandler>(
    State(state): State<AppState<H>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, axum::http::StatusCode> {
    // Security validation
    validate_security(&state, &headers, addr.ip())?;

    // Check Accept header
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !accept.contains("text/event-stream") {
        return Err(StatusCode::NOT_ACCEPTABLE);
    }

    // Session management
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let session_id = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok());

    let secure_session = get_or_create_session(&state, session_id, addr.ip(), user_agent)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;

    // Handle Last-Event-ID for resumability
    let last_event_id = headers.get("Last-Event-ID").and_then(|v| v.to_str().ok());

    // Create SSE stream
    let (tx, mut rx) = mpsc::unbounded_channel::<StoredEvent>();

    // Get or create session and register stream
    let mut sessions = state.sessions.write().await;
    let session = sessions
        .entry(secure_session.id.clone())
        .or_insert_with(|| Session::new(state.config.replay_buffer_size));

    // Replay events if Last-Event-ID provided
    let replay_events = if let Some(event_id) = last_event_id {
        session.replay_from(event_id)
    } else {
        Vec::new()
    };

    // Register this stream
    session.sse_senders.push(tx.clone());

    let endpoint_path = state.config.endpoint_path.clone();
    let session_id_for_stream = secure_session.id.clone();
    let keep_alive = state.config.keep_alive;

    drop(sessions);

    // Create SSE stream
    let stream = async_stream::stream! {
        // CRITICAL: First event MUST be "endpoint" per MCP 2025-06-18 spec
        let endpoint_url = format!("{}?sessionId={}", endpoint_path, session_id_for_stream);
        let endpoint_event = Event::default()
            .event("endpoint")
            .data(endpoint_url)
            .id(Uuid::new_v4().to_string());

        yield Ok::<Event, axum::Error>(endpoint_event);

        // Replay buffered events if resuming
        for event in replay_events {
            yield Ok(Event::default()
                .event(&event.event_type)
                .data(event.data)
                .id(event.id));
        }

        // Stream new events
        while let Some(event) = rx.recv().await {
            yield Ok(Event::default()
                .event(&event.event_type)
                .data(event.data)
                .id(event.id));
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(keep_alive)))
}

/// POST handler - Accepts JSON-RPC messages
///
/// Per MCP 2025-06-18:
/// - Accept header MUST include both application/json and text/event-stream
/// - For requests: Can return SSE stream OR JSON response
/// - For notifications/responses: Return 202 Accepted
async fn mcp_post_handler<H: turbomcp_protocol::JsonRpcHandler>(
    State(state): State<AppState<H>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Security validation
    if let Err(status) = validate_security(&state, &headers, addr.ip()) {
        return (
            status,
            HeaderMap::new(),
            Json(serde_json::json!({"error": "Forbidden"})),
        )
            .into_response();
    }

    // Protocol version validation
    // Default to latest supported version (2025-06-18) for best client compatibility
    let protocol_version = headers
        .get("MCP-Protocol-Version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("2025-06-18");

    if !matches!(protocol_version, "2025-06-18" | "2025-03-26" | "2024-11-05") {
        return (
            StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            Json(serde_json::json!({"error": "Unsupported protocol version"})),
        )
            .into_response();
    }

    // Parse JSON-RPC message
    let request: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                HeaderMap::new(),
                Json(serde_json::json!({"error": "Invalid JSON"})),
            )
                .into_response();
        }
    };

    // Session management
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let session_id = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok());

    // Check message type
    let is_notification = request.get("id").is_none();
    let is_response = request.get("result").is_some() || request.get("error").is_some();
    let is_request = !is_notification && !is_response;

    // Handle initialization specially
    if let Some(method) = request.get("method").and_then(|m| m.as_str())
        && method == "initialize"
    {
        return handle_initialize(&state, request, addr.ip(), user_agent, protocol_version).await;
    }

    // Validate session for non-initialization requests
    let secure_session =
        match get_or_create_session(&state, session_id, addr.ip(), user_agent).await {
            Ok(session) => session,
            Err(status) => {
                return (
                    status,
                    HeaderMap::new(),
                    Json(serde_json::json!({"error": "Forbidden"})),
                )
                    .into_response();
            }
        };

    // Per spec: Return 202 Accepted for notifications and responses
    if is_notification || is_response {
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            "Mcp-Session-Id",
            HeaderValue::from_str(&secure_session.id).unwrap(),
        );
        response_headers.insert(
            "MCP-Protocol-Version",
            HeaderValue::from_str(protocol_version).unwrap(),
        );

        // Broadcast to session's SSE streams
        broadcast_to_session(&state, &secure_session.id, request).await;

        return (
            StatusCode::ACCEPTED,
            response_headers,
            Json(serde_json::json!({})),
        )
            .into_response();
    }

    // For requests: Check Accept header to decide response type
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    let prefers_sse = accept.contains("text/event-stream");

    if is_request && prefers_sse {
        // Return SSE stream with response
        let (status, headers, body) =
            handle_request_with_sse(&state, &secure_session.id, request, protocol_version).await;
        return (status, headers, body).into_response();
    }

    // Default: Return JSON response
    // Delegate to handler (handler receives Value and returns Value)
    let response_value = state.handler.handle_request(request).await;

    // Build response headers
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "Mcp-Session-Id",
        HeaderValue::from_str(&secure_session.id).unwrap(),
    );
    response_headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_str(protocol_version).unwrap(),
    );

    (StatusCode::OK, response_headers, Json(response_value)).into_response()
}

/// DELETE handler - Terminates session
///
/// Per MCP 2025-06-18:
/// - Client sends DELETE with Mcp-Session-Id header
/// - Server responds 200 OK or 405 Method Not Allowed
async fn mcp_delete_handler<H: turbomcp_protocol::JsonRpcHandler>(
    State(state): State<AppState<H>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> StatusCode {
    // Security validation
    if validate_security(&state, &headers, addr.ip()).is_err() {
        return StatusCode::FORBIDDEN;
    }

    let session_id = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok());

    if let Some(session_id) = session_id {
        state.sessions.write().await.remove(session_id);
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Validate security (Origin header, rate limiting, etc.)
fn validate_security<H: turbomcp_protocol::JsonRpcHandler>(
    state: &AppState<H>,
    headers: &HeaderMap,
    client_ip: std::net::IpAddr,
) -> Result<(), StatusCode> {
    let security_headers = convert_headers(headers);

    state
        .security_validator
        .validate_request(&security_headers, client_ip)
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                client_ip = %client_ip,
                "Security validation failed"
            );
            StatusCode::from_u16(e.to_http_status()).unwrap_or(StatusCode::FORBIDDEN)
        })
}

/// Convert axum HeaderMap to SecurityHeaders
fn convert_headers(headers: &HeaderMap) -> SecurityHeaders {
    let mut security_headers = SecurityHeaders::new();
    for (key, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            security_headers.insert(key.to_string(), value_str.to_string());
        }
    }
    security_headers
}

/// Get or create secure session
async fn get_or_create_session<H: turbomcp_protocol::JsonRpcHandler>(
    state: &AppState<H>,
    session_id: Option<&str>,
    client_ip: std::net::IpAddr,
    user_agent: Option<&str>,
) -> Result<crate::security::SecureSessionInfo, axum::http::StatusCode> {
    match session_id {
        Some(id) => state
            .session_manager
            .validate_session(id, client_ip, user_agent)
            .or_else(|_| state.session_manager.create_session(client_ip, user_agent))
            .map_err(|_| StatusCode::FORBIDDEN),
        None => state
            .session_manager
            .create_session(client_ip, user_agent)
            .map_err(|_| StatusCode::FORBIDDEN),
    }
}

/// Handle initialize request
async fn handle_initialize<H: turbomcp_protocol::JsonRpcHandler>(
    state: &AppState<H>,
    request: serde_json::Value,
    client_ip: std::net::IpAddr,
    user_agent: Option<&str>,
    protocol_version: &str,
) -> Response {
    let secure_session = match state.session_manager.create_session(client_ip, user_agent) {
        Ok(session) => session,
        Err(_) => {
            return (
                StatusCode::FORBIDDEN,
                HeaderMap::new(),
                Json(serde_json::json!({"error": "Forbidden"})),
            )
                .into_response();
        }
    };

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "Mcp-Session-Id",
        HeaderValue::from_str(&secure_session.id).unwrap(),
    );
    response_headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_str(protocol_version).unwrap(),
    );

    // Get server info and capabilities from handler
    let server_info = state.handler.server_info();
    let capabilities = state.handler.capabilities();

    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "protocolVersion": protocol_version,
            "serverInfo": {
                "name": server_info.name,
                "version": server_info.version
            },
            "capabilities": capabilities
        },
        "id": request.get("id")
    });

    (StatusCode::OK, response_headers, Json(response)).into_response()
}

/// Broadcast message to session's SSE streams
async fn broadcast_to_session<H: turbomcp_protocol::JsonRpcHandler>(
    state: &AppState<H>,
    session_id: &str,
    message: serde_json::Value,
) {
    let mut sessions = state.sessions.write().await;

    if let Some(session) = sessions.get_mut(session_id) {
        let event = StoredEvent {
            id: Uuid::new_v4().to_string(),
            event_type: "message".to_string(),
            data: serde_json::to_string(&message).unwrap_or_default(),
        };

        session.broadcast_event(event);
    }
}

/// Handle request with SSE response
async fn handle_request_with_sse<H: turbomcp_protocol::JsonRpcHandler>(
    state: &AppState<H>,
    session_id: &str,
    request: serde_json::Value,
    protocol_version: &str,
) -> (StatusCode, HeaderMap, impl IntoResponse) {
    let (tx, mut rx) = mpsc::unbounded_channel::<StoredEvent>();

    // Process the request asynchronously and send result over SSE
    let handler = state.handler.clone();
    let request_clone = request.clone();
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        // Actually process the request!
        let response = handler.handle_request(request_clone).await;

        // Send the real response as SSE event
        let response_event = StoredEvent {
            id: Uuid::new_v4().to_string(),
            event_type: "message".to_string(),
            data: serde_json::to_string(&response).unwrap_or_default(),
        };

        tx_clone.send(response_event).ok();
    });

    // Register for session events
    let mut sessions = state.sessions.write().await;
    if let Some(session) = sessions.get_mut(session_id) {
        session.sse_senders.push(tx);
    }
    drop(sessions);

    let keep_alive = state.config.keep_alive;

    // Create SSE stream
    let stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            yield Ok::<Event, axum::Error>(Event::default()
                .event(&event.event_type)
                .data(event.data)
                .id(event.id));
        }
    };

    let mut response_headers = HeaderMap::new();
    response_headers.insert("Mcp-Session-Id", HeaderValue::from_str(session_id).unwrap());
    response_headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_str(protocol_version).unwrap(),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );

    (
        StatusCode::OK,
        response_headers,
        Sse::new(stream).keep_alive(KeepAlive::new().interval(keep_alive)),
    )
}

/// Run streamable HTTP server with a custom JSON-RPC handler
///
/// # Type Parameters
///
/// * `H` - JSON-RPC handler implementation (typically macro-generated)
///
/// # Arguments
///
/// * `config` - Transport configuration
/// * `handler` - JSON-RPC request handler
///
/// # Returns
///
/// Result indicating success or error
pub async fn run_server<H: turbomcp_protocol::JsonRpcHandler>(
    config: StreamableHttpConfig,
    handler: Arc<H>,
) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = config.endpoint_path.clone();
    let bind_addr = config.bind_addr.clone();
    let server_info = handler.server_info();

    let app = create_router(config, handler);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("🚀 MCP 2025-06-18 Compliant Streamable HTTP Transport Ready");
    tracing::info!("   Server: {} v{}", server_info.name, server_info.version);
    tracing::info!("   Listening: {}", bind_addr);
    tracing::info!("   Endpoint: {} (GET/POST/DELETE)", endpoint);
    tracing::info!("   Security: Origin validation, rate limiting, IP binding");
    tracing::info!("   Features: SSE streaming, message replay, session management");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_defaults() {
        let config = StreamableHttpConfig::default();
        assert_eq!(config.bind_addr, "127.0.0.1:8080");
        assert_eq!(config.endpoint_path, "/mcp");
        assert_eq!(config.keep_alive, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_session_replay() {
        let mut session = Session::new(10);

        // Add events
        for i in 0..5 {
            session.broadcast_event(StoredEvent {
                id: format!("event-{}", i),
                event_type: "message".to_string(),
                data: format!("data-{}", i),
            });
        }

        // Replay from event-2
        let replayed = session.replay_from("event-2");
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].id, "event-3");
        assert_eq!(replayed[1].id, "event-4");
    }

    #[tokio::test]
    async fn test_session_buffer_limit() {
        let mut session = Session::new(5);

        // Add more events than buffer size
        for i in 0..10 {
            session.broadcast_event(StoredEvent {
                id: format!("event-{}", i),
                event_type: "message".to_string(),
                data: format!("data-{}", i),
            });
        }

        // Should only keep last 5
        assert_eq!(session.event_buffer.len(), 5);
        assert_eq!(session.event_buffer[0].id, "event-5");
        assert_eq!(session.event_buffer[4].id, "event-9");
    }
}
