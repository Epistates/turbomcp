//! WebSocket Server Runtime with Bidirectional Support for ServerBuilder
//!
//! This module provides WebSocket transport with full MCP 2025-06-18 bidirectional support
//! for ServerBuilder pattern. It mirrors the HTTP implementation but leverages WebSocket's
//! native full-duplex capabilities.
//!
//! ## Architecture
//!
//! ```text
//! WebSocket Per-Connection Lifecycle
//! ═══════════════════════════════════
//!
//! Client connects (WebSocket upgrade)
//!          │
//!          ▼
//! ┌─────────────────────────────────────┐
//! │ Create WebSocketServerDispatcher    │
//! │ (wraps the WebSocket stream)        │
//! └──────────────────┬──────────────────┘
//!                    │
//!                    ▼
//! ┌─────────────────────────────────────┐
//! │ Use wrapper_factory to create       │
//! │ per-connection handler              │
//! └──────────────────┬──────────────────┘
//!                    │
//!                    ▼
//! ┌─────────────────────────────────────┐
//! │ Spawn concurrent tasks:             │
//! │  ├─► receive_loop (client→server)   │
//! │  └─► send_loop (server→client)      │
//! └─────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt, stream::SplitSink, stream::SplitStream};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use crate::routing::ServerRequestDispatcher;
use crate::{MessageId, ServerError, ServerResult};
use turbomcp_protocol::JsonRpcHandler;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::jsonrpc::{
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload, JsonRpcVersion,
    ResponseId,
};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult, PingRequest, PingResult,
};
use turbomcp_transport::security::{
    AuthConfig, OriginConfig, RateLimitConfig, SecurityError, SecurityValidator,
};

/// Pending server-initiated requests (for response correlation)
type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>;

/// WebSocket server dispatcher for server-initiated requests
///
/// This dispatcher handles server→client requests over the WebSocket connection,
/// sending JSON-RPC requests and awaiting responses through the bidirectional stream.
///
/// ## Implementation
///
/// Unlike HTTP (which uses SSE), WebSocket provides native full-duplex communication,
/// so server→client requests are sent directly over the same WebSocket connection
/// that receives client→server requests.
#[derive(Clone, Debug)]
pub struct WebSocketServerDispatcher {
    /// Channel to send messages to the WebSocket send loop
    sender: mpsc::UnboundedSender<WsMessage>,
    /// Pending server-initiated requests
    pending_requests: PendingRequests,
}

impl WebSocketServerDispatcher {
    /// Create a new WebSocket server dispatcher
    pub fn new(
        sender: mpsc::UnboundedSender<WsMessage>,
        pending_requests: PendingRequests,
    ) -> Self {
        Self {
            sender,
            pending_requests,
        }
    }

    /// Send a JSON-RPC request and await response
    ///
    /// ## MCP Compliance
    ///
    /// - Generates unique UUID request ID
    /// - Sends JSON-RPC 2.0 formatted request
    /// - Registers pending request for correlation
    /// - Awaits response with 60-second timeout
    async fn send_request<Req, Res>(&self, method: &str, params: Req) -> ServerResult<Res>
    where
        Req: serde::Serialize,
        Res: serde::de::DeserializeOwned,
    {
        // Generate unique request ID
        let request_id = Uuid::new_v4().to_string();

        // Create JSON-RPC request
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::String(request_id.clone()),
            method: method.to_string(),
            params: Some(
                serde_json::to_value(params).map_err(|e| ServerError::Handler {
                    message: format!("Failed to serialize request params: {}", e),
                    context: Some("WebSocket request serialization".to_string()),
                })?,
            ),
        };

        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();

        // Register pending request
        self.pending_requests
            .lock()
            .await
            .insert(request_id.clone(), response_tx);

        // Serialize and send request
        let request_json = serde_json::to_string(&request).map_err(|e| ServerError::Handler {
            message: format!("Failed to serialize JSON-RPC request: {}", e),
            context: Some("WebSocket request".to_string()),
        })?;

        self.sender
            .send(WsMessage::Text(request_json.into()))
            .map_err(|e| ServerError::Handler {
                message: format!("Failed to send WebSocket message: {}", e),
                context: Some("WebSocket closed".to_string()),
            })?;

        // Await response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(60), response_rx)
            .await
            .map_err(|_| ServerError::Handler {
                message: "Request timeout (60s)".to_string(),
                context: Some(format!("method: {}", method)),
            })?
            .map_err(|_| ServerError::Handler {
                message: "Response channel closed".to_string(),
                context: Some(format!("method: {}", method)),
            })?;

        // Parse response
        match response.payload {
            JsonRpcResponsePayload::Success { result } => {
                serde_json::from_value(result).map_err(|e| ServerError::Handler {
                    message: format!("Failed to deserialize response: {}", e),
                    context: Some(format!("method: {}", method)),
                })
            }
            JsonRpcResponsePayload::Error { error } => Err(ServerError::Handler {
                message: format!("Request failed: {}", error.message),
                context: Some(format!(
                    "MCP error code: {}, method: {}",
                    error.code, method
                )),
            }),
        }
    }
}

#[async_trait::async_trait]
impl ServerRequestDispatcher for WebSocketServerDispatcher {
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        self.send_request("elicitation/create", request).await
    }

    async fn send_ping(
        &self,
        request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        self.send_request("ping", request).await
    }

    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        self.send_request("sampling/createMessage", request).await
    }

    async fn send_list_roots(
        &self,
        request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        self.send_request("roots/list", request).await
    }

    fn supports_bidirectional(&self) -> bool {
        true
    }

    async fn get_client_capabilities(&self) -> ServerResult<Option<Value>> {
        Ok(None)
    }
}

/// Application state for WebSocket server
struct WebSocketAppState<H, W>
where
    H: JsonRpcHandler + Send + Sync + 'static,
    W: JsonRpcHandler + Send + Sync + 'static,
{
    /// Base handler instance
    base_handler: Arc<H>,
    /// Wrapper factory (base_handler + dispatcher → wrapped handler)
    wrapper_factory: Arc<dyn Fn(H, WebSocketServerDispatcher) -> W + Send + Sync>,
    /// Security validator
    security_validator: Arc<SecurityValidator>,
}

impl<H, W> Clone for WebSocketAppState<H, W>
where
    H: JsonRpcHandler + Send + Sync + 'static,
    W: JsonRpcHandler + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            base_handler: Arc::clone(&self.base_handler),
            wrapper_factory: Arc::clone(&self.wrapper_factory),
            security_validator: Arc::clone(&self.security_validator),
        }
    }
}

/// Handle a WebSocket connection with bidirectional communication
async fn handle_websocket<H, W>(
    socket: WebSocket,
    base_handler: H,
    wrapper_factory: impl Fn(H, WebSocketServerDispatcher) -> W,
) -> Result<(), Box<dyn std::error::Error>>
where
    H: JsonRpcHandler + Send + Sync + Clone + 'static,
    W: JsonRpcHandler + Send + Sync + 'static,
{
    let (ws_sender, ws_receiver) = socket.split();

    // Create channels for bidirectional communication
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<WsMessage>();
    let pending_requests: PendingRequests = Arc::new(Mutex::new(HashMap::new()));

    // Create WebSocket dispatcher for this connection
    let dispatcher = WebSocketServerDispatcher::new(outbound_tx.clone(), pending_requests.clone());

    // Create wrapped handler with dispatcher
    let handler = wrapper_factory(base_handler, dispatcher);

    // Spawn send loop (server→client messages)
    let send_task = tokio::spawn(send_loop(ws_sender, outbound_rx));

    // Spawn receive loop (client→server messages)
    let receive_task = tokio::spawn(receive_loop(
        ws_receiver,
        handler,
        outbound_tx,
        pending_requests,
    ));

    // Wait for either task to complete (connection close)
    tokio::select! {
        _ = send_task => {
            tracing::debug!("WebSocket send loop terminated");
        }
        _ = receive_task => {
            tracing::debug!("WebSocket receive loop terminated");
        }
    }

    Ok(())
}

/// Send loop: forwards messages from channel to WebSocket
async fn send_loop(
    mut sender: SplitSink<WebSocket, WsMessage>,
    mut outbound_rx: mpsc::UnboundedReceiver<WsMessage>,
) {
    while let Some(message) = outbound_rx.recv().await {
        if let Err(e) = sender.send(message).await {
            tracing::error!("Failed to send WebSocket message: {}", e);
            break;
        }
    }
}

/// Receive loop: processes incoming WebSocket messages
async fn receive_loop<H>(
    mut receiver: SplitStream<WebSocket>,
    handler: H,
    outbound_tx: mpsc::UnboundedSender<WsMessage>,
    pending_requests: PendingRequests,
) where
    H: JsonRpcHandler + Send + Sync + 'static,
{
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                // Parse JSON
                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("Failed to parse JSON: {}", e);
                        continue;
                    }
                };

                // Check if this is a response to server-initiated request
                if let Ok(response) = serde_json::from_value::<JsonRpcResponse>(value.clone()) {
                    let response_id = match &response.id {
                        ResponseId(Some(id)) => match id {
                            MessageId::String(s) => s.clone(),
                            MessageId::Number(n) => n.to_string(),
                            MessageId::Uuid(u) => u.to_string(),
                        },
                        _ => {
                            // Not a valid response ID, treat as request
                            handle_client_request(&handler, &outbound_tx, value).await;
                            continue;
                        }
                    };

                    // Check if we have a pending request with this ID
                    if let Some(tx) = pending_requests.lock().await.remove(&response_id) {
                        // This is a response to our server-initiated request
                        let _ = tx.send(response);
                        continue;
                    }
                }

                // Otherwise, treat as client→server request
                handle_client_request(&handler, &outbound_tx, value).await;
            }
            Ok(WsMessage::Close(_)) => {
                tracing::debug!("WebSocket connection closed by client");
                break;
            }
            Ok(WsMessage::Ping(data)) => {
                // Respond to ping with pong
                let _ = outbound_tx.send(WsMessage::Pong(data));
            }
            Ok(WsMessage::Pong(_)) => {
                // Keep-alive pong received
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
            _ => {
                // Ignore binary and other message types
            }
        }
    }
}

/// Handle a client→server request
async fn handle_client_request<H>(
    handler: &H,
    outbound_tx: &mpsc::UnboundedSender<WsMessage>,
    request: Value,
) where
    H: JsonRpcHandler,
{
    // Check if this is a notification (JSON-RPC 2.0 spec: no response for notifications)
    if let Ok(message) = serde_json::from_value::<JsonRpcMessage>(request.clone())
        && matches!(message, JsonRpcMessage::Notification(_))
    {
        // Process the notification but don't send a response
        let _ = handler.handle_request(request).await;
        tracing::debug!("Processed notification (no response sent per JSON-RPC 2.0 spec)");
        return;
    }

    // Process through handler
    let response = handler.handle_request(request).await;

    // Send response
    let response_json = match serde_json::to_string(&response) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!("Failed to serialize response: {}", e);
            return;
        }
    };

    if let Err(e) = outbound_tx.send(WsMessage::Text(response_json.into())) {
        tracing::error!("Failed to queue response: {}", e);
    }
}

/// WebSocket upgrade handler
async fn websocket_handler<H, W>(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    State(state): State<WebSocketAppState<H, W>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode>
where
    H: JsonRpcHandler + Send + Sync + Clone + 'static,
    W: JsonRpcHandler + Send + Sync + 'static,
{
    // Security validation
    validate_websocket_security(&state, &headers, addr.ip())?;

    tracing::info!("WebSocket connection from {}", addr);

    // Clone base handler and wrapper factory for this connection
    let base_handler = (*state.base_handler).clone();
    let wrapper_factory = Arc::clone(&state.wrapper_factory);

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_websocket(socket, base_handler, |h, d| (wrapper_factory)(h, d)).await
        {
            tracing::error!("WebSocket handler error: {}", e);
        }
    }))
}

/// Validate WebSocket security (Origin header, rate limiting)
fn validate_websocket_security<H, W>(
    state: &WebSocketAppState<H, W>,
    headers: &HeaderMap,
    client_ip: std::net::IpAddr,
) -> Result<(), StatusCode>
where
    H: JsonRpcHandler + Send + Sync + 'static,
    W: JsonRpcHandler + Send + Sync + 'static,
{
    // Convert headers
    let mut security_headers = HashMap::new();
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
            tracing::warn!(error = ?e, client_ip = %client_ip, "WebSocket security validation failed");
            match e {
                SecurityError::InvalidOrigin(_) => StatusCode::FORBIDDEN,
                SecurityError::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
                SecurityError::AuthenticationFailed(_) => StatusCode::UNAUTHORIZED,
                SecurityError::MessageTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
                _ => StatusCode::FORBIDDEN,
            }
        })
}

/// Run WebSocket server with bidirectional MCP support
///
/// This function creates an Axum-based WebSocket server that handles MCP protocol
/// messages with full bidirectional communication support for ServerBuilder.
///
/// ## Architecture
///
/// Each WebSocket connection:
/// 1. Creates a `WebSocketServerDispatcher` for server→client requests
/// 2. Wraps the base handler with the dispatcher (via wrapper_factory)
/// 3. Spawns concurrent send/receive loops
/// 4. Processes messages bidirectionally until connection closes
///
/// ## Parameters
///
/// * `base_handler` - The unwrapped server implementation
/// * `wrapper_factory` - Factory to create wrapper (base + dispatcher → wrapped)
/// * `addr` - Bind address (e.g. "127.0.0.1:8080")
/// * `path` - WebSocket endpoint path (e.g. "/ws")
pub async fn run_websocket<H, W, F>(
    base_handler: H,
    wrapper_factory: F,
    addr: String,
    path: String,
) -> Result<(), Box<dyn std::error::Error>>
where
    H: JsonRpcHandler + Send + Sync + Clone + 'static,
    W: JsonRpcHandler + Send + Sync + 'static,
    F: Fn(H, WebSocketServerDispatcher) -> W + Send + Sync + 'static,
{
    // Create security infrastructure
    let origin_config = OriginConfig {
        allowed_origins: std::collections::HashSet::new(),
        allow_localhost: true,
        allow_any: false,
    };

    let auth_config = AuthConfig::default();

    let rate_limit_config = Some(RateLimitConfig {
        max_requests: 100,
        window: std::time::Duration::from_secs(1),
        enabled: true,
    });

    let security_validator = Arc::new(SecurityValidator::new(
        origin_config,
        auth_config,
        rate_limit_config,
    ));

    let state = WebSocketAppState {
        base_handler: Arc::new(base_handler),
        wrapper_factory: Arc::new(wrapper_factory),
        security_validator,
    };

    // Get server info from base handler
    let server_info = state.base_handler.server_info();

    // Create router
    let app = Router::new()
        .route(&path, get(websocket_handler::<H, W>))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("🚀 MCP 2025-06-18 Compliant WebSocket Transport Ready");
    tracing::info!("   Server: {} v{}", server_info.name, server_info.version);
    tracing::info!("   Listening: {}", addr);
    tracing::info!("   Endpoint: {} (WebSocket upgrade)", path);
    tracing::info!("   Features: Native bidirectional, full-duplex JSON-RPC");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
