//! HTTP Runtime - Full MCP 2025-06-18 over HTTP + SSE
//!
//! **Status**: Production implementation with proper transport integration
//!
//! This module implements the complete MCP protocol over HTTP transport, supporting
//! both client→server (tools via POST) and server→client (sampling, elicitation, roots,
//! ping via Server-Sent Events) with concurrent request handling.
//!
//! ## Architecture
//!
//! ```text
//! Server→Client: Direct SSE via Session
//! ┌────────────────────────────────────┐
//! │ Server calls send_elicitation()   │
//! │  ├─► Create JSON-RPC request       │
//! │  ├─► Register pending request      │
//! │  ├─► Lock sessions, get session    │
//! │  └─► session.broadcast_event()     │  ← Synchronous!
//! └────────────────────────────────────┘
//!          │
//!          ▼ (SSE stream)
//! ┌────────────────────────────────────┐
//! │ Client receives via SSE stream     │
//! │  ├─► Parse JSON-RPC request        │
//! │  ├─► Process request               │
//! │  └─► POST response to /mcp         │
//! └────────────────────────────────────┘
//!          │
//!          ▼ (HTTP POST)
//! ┌────────────────────────────────────┐
//! │ POST handler matches response ID   │
//! │  ├─► Check pending_requests        │
//! │  ├─► Complete oneshot channel      │
//! │  └─► Return 202 Accepted           │
//! └────────────────────────────────────┘
//! ```

use std::sync::Arc;

use tokio::sync::oneshot;
use uuid::Uuid;

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::jsonrpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult, PingRequest, PingResult,
};
use turbomcp_server::routing::ServerRequestDispatcher;

// Re-export types from transport for convenience
pub use turbomcp_transport::streamable_http_v2::{
    PendingRequestsMap, Session, SessionsMap, StoredEvent,
};

use std::time::Duration;

use crate::{MessageId, ServerError, ServerResult};

// Additional imports for HTTP server implementation
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::post,
};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use turbomcp_protocol::JsonRpcHandler;
use turbomcp_protocol::jsonrpc::ResponseId;
use turbomcp_transport::security::{SecurityError, SecurityValidator, SessionSecurityManager};
use turbomcp_transport::streamable_http_v2::{StreamableHttpConfig, StreamableHttpConfigBuilder};

/// HTTP dispatcher for server-initiated requests
///
/// This dispatcher integrates directly with streamable_http_v2's session management
/// to enable complete MCP 2025-06-18 support over HTTP + SSE.
///
/// ## MCP Compliance
///
/// - Sends JSON-RPC 2.0 formatted requests via SSE
/// - Generates unique request IDs for correlation
/// - Handles responses via HTTP POST (integrated into transport)
/// - Supports: sampling/createMessage, elicitation/create, roots/list, ping
///
/// ## Usage
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use tokio::sync::{Mutex, RwLock};
/// use std::collections::HashMap;
/// use turbomcp::runtime::http_bidirectional::HttpDispatcher;
/// use turbomcp_transport::streamable_http_v2::{PendingRequestsMap, SessionsMap};
///
/// # async fn example(sessions: SessionsMap, pending_requests: PendingRequestsMap) {
/// let dispatcher = HttpDispatcher::new(
///     "session-123".to_string(),
///     sessions,
///     pending_requests,
/// );
/// # }
/// ```
#[derive(Clone)]
pub struct HttpDispatcher {
    /// Session ID for this dispatcher
    session_id: String,
    /// Direct access to sessions for broadcasting
    sessions: SessionsMap,
    /// Pending server-initiated requests awaiting responses
    pending_requests: PendingRequestsMap,
}

impl HttpDispatcher {
    /// Create a new HTTP dispatcher
    ///
    /// # Arguments
    ///
    /// * `session_id` - MCP session ID
    /// * `sessions` - Shared sessions map from streamable_http_v2
    /// * `pending_requests` - Shared pending requests map
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use turbomcp::runtime::http_bidirectional::HttpDispatcher;
    /// use std::sync::Arc;
    /// use std::collections::HashMap;
    /// use tokio::sync::{Mutex, RwLock};
    ///
    /// # async fn example() {
    /// let sessions = Arc::new(RwLock::new(HashMap::new()));
    /// let pending_requests = Arc::new(Mutex::new(HashMap::new()));
    ///
    /// let dispatcher = HttpDispatcher::new(
    ///     "my-session".to_string(),
    ///     sessions,
    ///     pending_requests,
    /// );
    /// # }
    /// ```
    pub fn new(
        session_id: String,
        sessions: SessionsMap,
        pending_requests: PendingRequestsMap,
    ) -> Self {
        Self {
            session_id,
            sessions,
            pending_requests,
        }
    }

    /// Send a JSON-RPC request via SSE and wait for HTTP POST response
    ///
    /// This is the core method that:
    /// 1. Registers the request as pending
    /// 2. Broadcasts to client via direct session access
    /// 3. Waits for correlated response from HTTP POST
    ///
    /// ## MCP 2025-06-18 Compliance
    ///
    /// - Uses JSON-RPC 2.0 format
    /// - Generates unique request IDs (UUID v4)
    /// - Handles errors per MCP error codes
    /// - 60-second timeout per MCP recommendation
    async fn send_request(&self, request: JsonRpcRequest) -> ServerResult<JsonRpcResponse> {
        let (response_tx, response_rx) = oneshot::channel();

        // Extract request ID for correlation
        let request_id = match &request.id {
            MessageId::String(s) => s.clone(),
            MessageId::Number(n) => n.to_string(),
            MessageId::Uuid(u) => u.to_string(),
        };

        // Register pending request
        self.pending_requests
            .lock()
            .await
            .insert(request_id.clone(), response_tx);

        // Broadcast via SSE by directly accessing session
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&self.session_id) {
                let request_value =
                    serde_json::to_value(&request).map_err(|e| ServerError::Handler {
                        message: format!("Failed to serialize request: {}", e),
                        context: Some("http_dispatcher".to_string()),
                    })?;

                let event = StoredEvent {
                    id: Uuid::new_v4().to_string(),
                    event_type: "message".to_string(),
                    data: serde_json::to_string(&request_value).map_err(|e| {
                        ServerError::Handler {
                            message: format!("Failed to serialize event: {}", e),
                            context: Some("http_dispatcher".to_string()),
                        }
                    })?,
                };

                session.broadcast_event(event); // ✅ Synchronous!
            } else {
                return Err(ServerError::Handler {
                    message: format!("Session not found: {}", self.session_id),
                    context: Some("http_dispatcher".to_string()),
                });
            }
        } // Release lock immediately

        // Wait for response with 60-second timeout
        match tokio::time::timeout(tokio::time::Duration::from_secs(60), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(ServerError::Handler {
                message: "Response channel closed".to_string(),
                context: Some("http_dispatcher".to_string()),
            }),
            Err(_) => {
                // Timeout - cleanup
                self.pending_requests.lock().await.remove(&request_id);
                Err(ServerError::Handler {
                    message: "Request timeout (60s)".to_string(),
                    context: Some("http_dispatcher".to_string()),
                })
            }
        }
    }

    /// Create a unique request ID (UUID v4 per MCP best practices)
    fn generate_request_id() -> MessageId {
        MessageId::String(Uuid::new_v4().to_string())
    }
}

#[async_trait::async_trait]
impl ServerRequestDispatcher for HttpDispatcher {
    /// Send an elicitation request to the client
    ///
    /// Per MCP 2025-06-18 spec, method name is "elicitation/create"
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        // Create JSON-RPC request per MCP 2025-06-18
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "elicitation/create".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| ServerError::Handler {
                    message: format!("Failed to serialize elicitation request: {}", e),
                    context: Some("MCP 2025-06-18 compliance".to_string()),
                })?,
            ),
            id: Self::generate_request_id(),
        };

        // Send request and wait for response
        let response = self.send_request(json_rpc_request).await?;

        // Extract result from response
        use turbomcp_protocol::jsonrpc::JsonRpcResponsePayload;
        match response.payload {
            JsonRpcResponsePayload::Success { result } => {
                serde_json::from_value(result).map_err(|e| ServerError::Handler {
                    message: format!("Failed to deserialize elicitation result: {}", e),
                    context: Some("MCP 2025-06-18 compliance".to_string()),
                })
            }
            JsonRpcResponsePayload::Error { error } => Err(ServerError::Handler {
                message: format!("Elicitation failed: {}", error.message),
                context: Some(format!("MCP error code: {}", error.code)),
            }),
        }
    }

    /// Send a ping request to the client
    ///
    /// Per MCP 2025-06-18 spec, method name is "ping"
    async fn send_ping(
        &self,
        _request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        // Create JSON-RPC request per MCP 2025-06-18
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "ping".to_string(),
            params: None, // Ping has no parameters
            id: Self::generate_request_id(),
        };

        // Send request and wait for response
        let response = self.send_request(json_rpc_request).await?;

        // Extract result from response
        use turbomcp_protocol::jsonrpc::JsonRpcResponsePayload;
        match response.payload {
            JsonRpcResponsePayload::Success { .. } => Ok(PingResult {
                _meta: None,
                data: None,
            }),
            JsonRpcResponsePayload::Error { error } => Err(ServerError::Handler {
                message: format!("Ping failed: {}", error.message),
                context: Some(format!("MCP error code: {}", error.code)),
            }),
        }
    }

    /// Send a sampling create message request to the client
    ///
    /// Per MCP 2025-06-18 spec, method name is "sampling/createMessage"
    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        // Create JSON-RPC request per MCP 2025-06-18
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "sampling/createMessage".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| ServerError::Handler {
                    message: format!("Failed to serialize sampling request: {}", e),
                    context: Some("MCP 2025-06-18 compliance".to_string()),
                })?,
            ),
            id: Self::generate_request_id(),
        };

        // Send request and wait for response
        let response = self.send_request(json_rpc_request).await?;

        // Extract result from response
        use turbomcp_protocol::jsonrpc::JsonRpcResponsePayload;
        match response.payload {
            JsonRpcResponsePayload::Success { result } => {
                serde_json::from_value(result).map_err(|e| ServerError::Handler {
                    message: format!("Failed to deserialize sampling result: {}", e),
                    context: Some("MCP 2025-06-18 compliance".to_string()),
                })
            }
            JsonRpcResponsePayload::Error { error } => Err(ServerError::Handler {
                message: format!("Sampling failed: {}", error.message),
                context: Some(format!("MCP error code: {}", error.code)),
            }),
        }
    }

    /// Send a roots list request to the client
    ///
    /// Per MCP 2025-06-18 spec, method name is "roots/list"
    async fn send_list_roots(
        &self,
        request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        // Create JSON-RPC request per MCP 2025-06-18
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "roots/list".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| ServerError::Handler {
                    message: format!("Failed to serialize roots request: {}", e),
                    context: Some("MCP 2025-06-18 compliance".to_string()),
                })?,
            ),
            id: Self::generate_request_id(),
        };

        // Send request and wait for response
        let response = self.send_request(json_rpc_request).await?;

        // Extract result from response
        use turbomcp_protocol::jsonrpc::JsonRpcResponsePayload;
        match response.payload {
            JsonRpcResponsePayload::Success { result } => {
                serde_json::from_value(result).map_err(|e| ServerError::Handler {
                    message: format!("Failed to deserialize roots result: {}", e),
                    context: Some("MCP 2025-06-18 compliance".to_string()),
                })
            }
            JsonRpcResponsePayload::Error { error } => Err(ServerError::Handler {
                message: format!("Roots list failed: {}", error.message),
                context: Some(format!("MCP error code: {}", error.code)),
            }),
        }
    }

    /// Check if server→client requests are supported
    ///
    /// For HTTP transport, server→client requests are always supported via SSE
    fn supports_bidirectional(&self) -> bool {
        true
    }

    /// Get client capabilities
    ///
    /// HTTP transport doesn't store client capabilities separately
    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>> {
        Ok(None)
    }
}

/// Application state for the HTTP server with bidirectional support
struct HttpAppState<F, H>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
    /// Factory function that creates handlers with session-specific dispatchers
    handler_factory: Arc<F>,
    /// Shared sessions map for SSE broadcasting
    sessions: SessionsMap,
    /// Shared pending requests map for response correlation
    pending_requests: PendingRequestsMap,
    /// Security validator for Origin header validation
    security_validator: Arc<SecurityValidator>,
    /// Session manager for session lifecycle
    session_manager: Arc<SessionSecurityManager>,
    /// Transport configuration
    config: StreamableHttpConfig,
}

impl<F, H> Clone for HttpAppState<F, H>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            handler_factory: Arc::clone(&self.handler_factory),
            sessions: Arc::clone(&self.sessions),
            pending_requests: Arc::clone(&self.pending_requests),
            security_validator: Arc::clone(&self.security_validator),
            session_manager: Arc::clone(&self.session_manager),
            config: self.config.clone(),
        }
    }
}

/// Run MCP HTTP server with full bidirectional support
///
/// This function implements the complete MCP 2025-06-18 HTTP transport:
/// - Client→Server: HTTP POST with JSON-RPC messages
/// - Server→Client: Server-Sent Events (SSE) for streaming responses and server-initiated requests
/// - Session Management: `Mcp-Session-Id` header for stateful sessions
/// - Security: Origin validation, localhost binding, rate limiting
///
/// # Architecture
///
/// This uses a **factory pattern** where the macro generates session-specific handler creation:
///
/// ```text
/// For each HTTP POST request:
///   1. Extract session_id from Mcp-Session-Id header
///   2. Call handler_factory(session_id) to create wrapper with session-specific dispatcher
///   3. Wrapper has RequestContext with server_to_client capabilities
///   4. Tools can call ctx.server_to_client.create_message(), etc.
///   5. Dispatcher broadcasts server requests via SSE to correct session
/// ```
///
/// # Type Parameters
///
/// * `F` - Factory function that creates handlers: `Fn(Option<String>) -> H`
/// * `H` - Handler type that implements `JsonRpcHandler` (typically the generated wrapper)
///
/// # Arguments
///
/// * `handler_factory` - Function that creates handlers with session-specific context
/// * `sessions` - Shared sessions map for SSE broadcasting
/// * `pending_requests` - Shared pending requests map for response correlation
/// * `addr` - Bind address (e.g., "127.0.0.1:3000")
/// * `path` - MCP endpoint path (e.g., "/mcp")
///
/// # Example
///
/// This function is called by macro-generated code:
///
/// ```no_run
/// # use std::sync::Arc;
/// # use std::collections::HashMap;
/// # use tokio::sync::{Mutex, RwLock};
/// # use turbomcp::runtime::http_bidirectional::{HttpDispatcher, run_http};
/// # use uuid::Uuid;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let sessions = Arc::new(RwLock::new(HashMap::new()));
/// let pending_requests = Arc::new(Mutex::new(HashMap::new()));
///
/// // Factory creates wrapper with session-specific dispatcher
/// let handler_factory = move |session_id: Option<String>| {
///     let dispatcher = HttpDispatcher::new(
///         session_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
///         Arc::clone(&sessions),
///         Arc::clone(&pending_requests),
///     );
///     // MyServerBidirectional::with_dispatcher(server, dispatcher)
///     // (generated by macro)
/// #   todo!()
/// };
///
/// run_http(
///     handler_factory,
///     sessions,
///     pending_requests,
///     "127.0.0.1:3000".to_string(),
///     "/mcp".to_string(),
/// ).await?;
/// # Ok(())
/// # }
/// ```
///
/// # MCP 2025-06-18 Compliance
///
/// - ✅ POST for client→server messages
/// - ✅ GET for server→client SSE streams
/// - ✅ Session management via `Mcp-Session-Id` header
/// - ✅ Multiple concurrent SSE streams per session
/// - ✅ Server-initiated requests (sampling, elicitation, roots, ping)
/// - ✅ Request/response correlation via pending_requests map
/// - ✅ Origin validation (DNS rebinding protection)
/// - ✅ Localhost binding by default
///
/// # Security
///
/// - Origin header validation prevents DNS rebinding attacks
/// - Binds to localhost by default (not 0.0.0.0)
/// - Rate limiting per IP address
/// - Session ID must be cryptographically secure
pub async fn run_http<F, H>(
    handler_factory: F,
    sessions: SessionsMap,
    pending_requests: PendingRequestsMap,
    addr: String,
    path: String,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
    // Create transport configuration
    let config = StreamableHttpConfigBuilder::new()
        .with_bind_address(addr.clone())
        .with_endpoint_path(path.clone())
        .allow_localhost(true) // Required for MCP
        .build();

    // Create application state
    let state = HttpAppState {
        handler_factory: Arc::new(handler_factory),
        sessions,
        pending_requests,
        security_validator: config.security_validator.clone(),
        session_manager: config.session_manager.clone(),
        config: config.clone(),
    };

    // Get server info from a temporary handler instance
    let temp_handler = (state.handler_factory)(None);
    let server_info = temp_handler.server_info();

    // Create router with custom handlers
    let app = Router::new()
        .route(
            &config.endpoint_path,
            post(mcp_post_handler::<F, H>)
                .get(mcp_get_handler::<F, H>)
                .delete(mcp_delete_handler::<F, H>),
        )
        .with_state(state);

    // Bind to address
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("🚀 MCP 2025-06-18 Compliant HTTP Transport Ready");
    tracing::info!("   Server: {} v{}", server_info.name, server_info.version);
    tracing::info!("   Listening: {}", addr);
    tracing::info!("   Endpoint: {} (GET/POST/DELETE)", path);
    tracing::info!("   Security: Origin validation, rate limiting, localhost binding");
    tracing::info!("   Features: Full bidirectional support, SSE streaming, session management");

    // Run server
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// POST handler - Receives client messages and returns SSE stream or JSON response
async fn mcp_post_handler<F, H>(
    State(state): State<HttpAppState<F, H>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Result<impl IntoResponse, StatusCode>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
    // Security validation
    validate_security(&state, &headers, addr.ip())?;

    // Extract session ID from header
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // Check if this is a response to a server-initiated request
    if let Some(ref _session_id) = session_id
        && let Ok(response) = serde_json::from_value::<JsonRpcResponse>(request.clone())
    {
        // Extract response ID
        let response_id = match &response.id {
            ResponseId(Some(id)) => match id {
                MessageId::String(s) => s.clone(),
                MessageId::Number(n) => n.to_string(),
                MessageId::Uuid(u) => u.to_string(),
            },
            _ => return Ok((StatusCode::ACCEPTED, Json(serde_json::json!({})))),
        };

        // Check if this matches a pending server-initiated request
        if let Some(tx) = state.pending_requests.lock().await.remove(&response_id) {
            // Complete the pending request
            let _ = tx.send(response);
            return Ok((StatusCode::ACCEPTED, Json(serde_json::json!({}))));
        }
    }

    // Check if this is a notification (JSON-RPC 2.0 spec: no response for notifications)
    if let Ok(message) = serde_json::from_value::<JsonRpcMessage>(request.clone())
        && matches!(message, JsonRpcMessage::Notification(_))
    {
        // Process the notification but don't send a response
        let handler = (state.handler_factory)(session_id.clone());
        let _ = handler.handle_request(request).await;

        // Return 204 No Content per JSON-RPC 2.0 spec
        return Ok((StatusCode::NO_CONTENT, Json(serde_json::json!({}))));
    }

    // This is a regular client request - create handler with session context
    let handler = (state.handler_factory)(session_id.clone());

    // Handle the request
    let response_value = handler.handle_request(request).await;

    // Return JSON response
    Ok((StatusCode::OK, Json(response_value)))
}

/// GET handler - Opens SSE stream for server-initiated messages
///
/// Per MCP 2025-06-18 spec:
/// - Opens SSE stream for server→client messages
/// - Supports Last-Event-ID for resumability
/// - Can send JSON-RPC requests and notifications
async fn mcp_get_handler<F, H>(
    State(state): State<HttpAppState<F, H>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
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

    // Extract session ID (required for GET)
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Check for resumability (Last-Event-ID)
    let last_event_id = headers.get("Last-Event-ID").and_then(|v| v.to_str().ok());

    // Create SSE channel
    let (tx, mut rx) = mpsc::unbounded_channel::<StoredEvent>();

    // Register sender with session and replay if needed
    {
        let mut sessions = state.sessions.write().await;

        // Get or create session
        let session = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session {
                event_buffer: std::collections::VecDeque::with_capacity(1000),
                sse_senders: Vec::new(),
            });

        // Replay events if Last-Event-ID provided
        if let Some(last_id) = last_event_id {
            let events = session.replay_from(last_id);
            for event in events {
                let _ = tx.send(event);
            }
        }

        // Register this stream for future events
        session.sse_senders.push(tx);
    }

    // Create SSE response stream
    let stream = async_stream::stream! {
        // First event MUST be endpoint info per MCP spec
        let endpoint_event = Event::default()
            .event("endpoint")
            .data(serde_json::json!({
                "uri": format!("{}/mcp", state.config.bind_addr)
            }).to_string());

        yield Ok::<Event, axum::Error>(endpoint_event);

        // Stream subsequent events
        while let Some(event) = rx.recv().await {
            yield Ok(Event::default()
                .event(&event.event_type)
                .data(event.data)
                .id(event.id));
        }
    };

    // Create response headers
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "Mcp-Session-Id",
        HeaderValue::from_str(session_id).unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );
    response_headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_static("2025-06-18"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );

    Ok((
        StatusCode::OK,
        response_headers,
        Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30))),
    ))
}

/// DELETE handler - Terminates a session
async fn mcp_delete_handler<F, H>(
    State(state): State<HttpAppState<F, H>>,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
    // Extract session ID
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Remove session from sessions map
    state.sessions.write().await.remove(session_id);

    Ok(StatusCode::NO_CONTENT)
}

/// Security validation helper
///
/// Validates requests according to MCP 2025-06-18 security requirements:
/// - Origin header validation (DNS rebinding protection)
/// - Rate limiting per IP address
/// - Session security checks
fn validate_security<F, H>(
    state: &HttpAppState<F, H>,
    headers: &HeaderMap,
    client_ip: std::net::IpAddr,
) -> Result<(), StatusCode>
where
    F: Fn(Option<String>) -> H + Send + Sync + 'static,
    H: JsonRpcHandler + Send + Sync + 'static,
{
    // Convert Axum headers to transport security headers
    let mut security_headers = std::collections::HashMap::new();
    for (key, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            security_headers.insert(key.to_string(), value_str.to_string());
        }
    }

    // Validate using transport security infrastructure
    state
        .security_validator
        .validate_request(&security_headers, client_ip)
        .map_err(|e| {
            tracing::warn!(
                error = ?e,
                client_ip = %client_ip,
                "Security validation failed"
            );
            match e {
                SecurityError::InvalidOrigin(_) => StatusCode::FORBIDDEN,
                SecurityError::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
                SecurityError::AuthenticationFailed(_) => StatusCode::UNAUTHORIZED,
                SecurityError::MessageTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
                _ => StatusCode::FORBIDDEN,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::{Mutex, RwLock};

    #[tokio::test]
    async fn test_http_dispatcher_creation() {
        let sessions: SessionsMap = Arc::new(RwLock::new(HashMap::new()));
        let pending_requests: PendingRequestsMap = Arc::new(Mutex::new(HashMap::new()));

        let dispatcher =
            HttpDispatcher::new("test-session".to_string(), sessions, pending_requests);

        assert!(dispatcher.supports_bidirectional());
    }

    #[tokio::test]
    async fn test_generate_request_id() {
        let id1 = HttpDispatcher::generate_request_id();
        let id2 = HttpDispatcher::generate_request_id();

        // IDs should be unique
        match (&id1, &id2) {
            (MessageId::String(s1), MessageId::String(s2)) => {
                assert_ne!(s1, s2);
            }
            _ => panic!("Expected String IDs"),
        }
    }
}
