//! Runtime components for bidirectional transport
//!
//! This module provides unified bidirectional communication support for all
//! duplex transports (STDIO, TCP, Unix Socket, HTTP, WebSocket) with full MCP 2025-11-25 compliance.
//!
//! ## Architecture
//!
//! **Generic Abstraction**: `TransportDispatcher<T>` works with any `Transport`
//! - Sends server-initiated requests via transport
//! - Correlates responses with pending requests
//! - Implements `ServerRequestDispatcher` trait
//!
//! **Specialized Implementations**:
//! - `StdioDispatcher`: Optimized for stdin/stdout (line-delimited JSON)
//! - `TransportDispatcher<TcpTransport>`: For TCP sockets
//! - `TransportDispatcher<UnixTransport>`: For Unix domain sockets
//! - `http::HttpDispatcher`: For HTTP + SSE sessions (feature-gated)
//! - WebSocket: Uses transport layer's bidirectional infrastructure (`turbomcp_transport::axum::WebSocketDispatcher`)
//!
//! All share the same request correlation and error handling logic.

// HTTP bidirectional support (feature-gated)
#[cfg(feature = "http")]
pub mod http;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinSet;

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult, PingRequest, PingResult,
};

use crate::routing::{RequestRouter, ServerRequestDispatcher};
use crate::{McpError, ServerResult};

type MessageId = turbomcp_protocol::MessageId;

/// STDIO dispatcher for server-initiated requests
///
/// This dispatcher implements the MCP 2025-11-25 specification for stdio transport,
/// allowing servers to make requests to clients (server→client capability).
#[derive(Clone)]
pub struct StdioDispatcher {
    /// Channel for sending messages to stdout writer
    request_tx: mpsc::UnboundedSender<StdioMessage>,
    /// Pending server-initiated requests awaiting responses
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
}

impl std::fmt::Debug for StdioDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioDispatcher")
            .field("has_request_tx", &true)
            .field("pending_count", &"<mutex>")
            .finish()
    }
}

/// Internal message type for STDIO transport
#[derive(Debug)]
pub enum StdioMessage {
    /// Server request to be sent to client
    ServerRequest {
        /// The JSON-RPC request
        request: JsonRpcRequest,
    },
    /// Shutdown signal
    Shutdown,
}

impl StdioDispatcher {
    /// Create a new STDIO dispatcher
    pub fn new(request_tx: mpsc::UnboundedSender<StdioMessage>) -> Self {
        Self {
            request_tx,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send a JSON-RPC request and wait for response
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

        // Send to stdout writer
        self.request_tx
            .send(StdioMessage::ServerRequest { request })
            .map_err(|e| {
                McpError::internal(format!("Failed to send request to stdout: {}", e))
                    .with_operation("stdio_dispatcher")
            })?;

        // Wait for response with timeout (60 seconds per MCP recommendation)
        match tokio::time::timeout(tokio::time::Duration::from_secs(60), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                Err(McpError::internal("Response channel closed")
                    .with_operation("stdio_dispatcher"))
            }
            Err(_) => {
                // Timeout - remove from pending
                self.pending_requests.lock().await.remove(&request_id);
                Err(McpError::timeout("Request timeout (60s)")
                    .with_operation("stdio_dispatcher"))
            }
        }
    }

    /// Generate a unique request ID
    fn generate_request_id() -> MessageId {
        MessageId::String(uuid::Uuid::new_v4().to_string())
    }
}

#[async_trait::async_trait]
impl ServerRequestDispatcher for StdioDispatcher {
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "elicitation/create".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| {
                    McpError::internal(format!("Failed to serialize elicitation request: {}", e))
                        .with_operation("MCP compliance")
                })?,
            ),
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| {
                McpError::internal(format!("Invalid elicitation response format: {}", e))
                    .with_operation("MCP compliance")
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal(
                    "Invalid elicitation response: missing result and error",
                )
                .with_operation("MCP compliance"),
            )
        }
    }

    async fn send_ping(
        &self,
        _request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "ping".to_string(),
            params: None,
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if response.result().is_some() {
            Ok(PingResult {
                data: None,
                _meta: None,
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal("Invalid ping response")
                    .with_operation("MCP compliance"),
            )
        }
    }

    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "sampling/createMessage".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| {
                    McpError::internal(format!("Failed to serialize sampling request: {}", e))
                        .with_operation("MCP compliance")
                })?,
            ),
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| {
                McpError::internal(format!("Invalid sampling response format: {}", e))
                    .with_operation("MCP compliance")
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal("Invalid sampling response: missing result and error")
                    .with_operation("MCP compliance"),
            )
        }
    }

    async fn send_list_roots(
        &self,
        _request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "roots/list".to_string(),
            params: None,
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| {
                McpError::internal(format!("Invalid roots response format: {}", e))
                    .with_operation("MCP compliance")
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal("Invalid roots response: missing result and error")
                    .with_operation("MCP compliance"),
            )
        }
    }

    fn supports_bidirectional(&self) -> bool {
        true
    }

    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>> {
        Ok(None)
    }
}

/// Run MCP server over STDIO transport with full bidirectional support
///
/// This runtime implements the complete MCP 2025-11-25 stdio protocol:
/// - Reads JSON-RPC from stdin (client requests AND server response correlations)
/// - Writes JSON-RPC to stdout (server responses AND server requests)
/// - Maintains request/response correlation
/// - Handles errors per MCP spec
/// - Manages task lifecycle with JoinSet for clean shutdown
pub async fn run_stdio_bidirectional(
    router: Arc<RequestRouter>,
    dispatcher: StdioDispatcher,
    mut request_rx: mpsc::UnboundedReceiver<StdioMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    let stdout = Arc::new(Mutex::new(stdout));
    let pending_requests = Arc::clone(&dispatcher.pending_requests);

    // ✅ Create JoinSet to manage all spawned tasks
    let mut tasks = JoinSet::new();

    // ✅ Spawn stdout writer task and store handle in JoinSet
    let stdout_writer = Arc::clone(&stdout);
    tasks.spawn(async move {
        while let Some(msg) = request_rx.recv().await {
            match msg {
                StdioMessage::ServerRequest { request } => {
                    if let Ok(json) = serde_json::to_string(&request) {
                        let mut stdout = stdout_writer.lock().await;
                        let _ = stdout.write_all(json.as_bytes()).await;
                        let _ = stdout.write_all(b"\n").await;
                        let _ = stdout.flush().await;
                    }
                }
                StdioMessage::Shutdown => break,
            }
        }
    });

    // Main stdin reader loop
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;
                }

                // Try parsing as JSON-RPC response first (for server-initiated request responses)
                if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) {
                    let request_id = match &response.id {
                        turbomcp_protocol::jsonrpc::ResponseId(Some(id)) => match id {
                            MessageId::String(s) => s.clone(),
                            MessageId::Number(n) => n.to_string(),
                            MessageId::Uuid(u) => u.to_string(),
                        },
                        _ => continue,
                    };

                    if let Some(tx) = pending_requests.lock().await.remove(&request_id) {
                        let _ = tx.send(response);
                    }
                    continue;
                }

                // Try parsing as JSON-RPC request (client-initiated)
                if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&line) {
                    let router = Arc::clone(&router);
                    let stdout = Arc::clone(&stdout);

                    // ✅ Spawn request handler and store handle in JoinSet
                    tasks.spawn(async move {
                        // Create properly configured context with server-to-client capabilities
                        let ctx = router.create_context(None, None, None);
                        let response = router.route(request, ctx).await;

                        if let Ok(json) = serde_json::to_string(&response) {
                            let mut stdout = stdout.lock().await;
                            let _ = stdout.write_all(json.as_bytes()).await;
                            let _ = stdout.write_all(b"\n").await;
                            let _ = stdout.flush().await;
                        }
                    });
                }
            }
            Err(_) => break,
        }
    }

    // ✅ GRACEFUL SHUTDOWN: Wait for all tasks to complete
    tracing::debug!(
        "STDIO dispatcher shutting down, waiting for {} tasks",
        tasks.len()
    );

    // Signal writer task to shutdown by dropping the channel
    // The request_rx.recv() in the writer task will return None, causing it to exit
    drop(dispatcher);

    // Wait for all tasks to complete with timeout
    let shutdown_timeout = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    while let Some(result) = tokio::time::timeout(
        shutdown_timeout.saturating_sub(start.elapsed()),
        tasks.join_next(),
    )
    .await
    .ok()
    .flatten()
    {
        match result {
            Ok(()) => {
                tracing::debug!("Task completed successfully during shutdown");
            }
            Err(e) if e.is_panic() => {
                tracing::warn!("Task panicked during shutdown: {:?}", e);
            }
            Err(e) if e.is_cancelled() => {
                tracing::debug!("Task was cancelled during shutdown");
            }
            Err(e) => {
                tracing::debug!("Task error during shutdown: {:?}", e);
            }
        }
    }

    // ✅ Abort remaining tasks if timeout occurred
    if !tasks.is_empty() {
        tracing::warn!(
            "Aborting {} tasks due to shutdown timeout ({}s)",
            tasks.len(),
            shutdown_timeout.as_secs()
        );
        tasks.shutdown().await;
    }

    tracing::debug!("STDIO dispatcher shutdown complete");
    Ok(())
}

// ============================================================================
// Generic Transport Dispatcher (TCP, Unix Socket, and other Transport impls)
// ============================================================================

/// Generic dispatcher for any Transport implementation
///
/// This provides bidirectional MCP support for any transport that implements
/// the `Transport` trait. Unlike `StdioDispatcher` which directly reads/writes
/// stdin/stdout, this uses the Transport trait's `send()` and `receive()` methods.
///
/// **Usage**:
/// ```rust,ignore
/// use turbomcp_transport::TcpTransport;
/// use turbomcp_server::runtime::TransportDispatcher;
///
/// let addr = "127.0.0.1:8080".parse().unwrap();
/// let transport = TcpTransport::new_server(addr);
/// let dispatcher = TransportDispatcher::new(transport);
/// ```
pub struct TransportDispatcher<T>
where
    T: turbomcp_transport::Transport,
{
    /// The underlying transport
    transport: Arc<T>,
    /// Pending server-initiated requests awaiting responses
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
}

// Manual Clone implementation: Arc cloning doesn't require T: Clone
impl<T> Clone for TransportDispatcher<T>
where
    T: turbomcp_transport::Transport,
{
    fn clone(&self) -> Self {
        Self {
            transport: Arc::clone(&self.transport),
            pending_requests: Arc::clone(&self.pending_requests),
        }
    }
}

impl<T> std::fmt::Debug for TransportDispatcher<T>
where
    T: turbomcp_transport::Transport,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportDispatcher")
            .field("transport_type", &self.transport.transport_type())
            .field("pending_count", &"<mutex>")
            .finish()
    }
}

impl<T> TransportDispatcher<T>
where
    T: turbomcp_transport::Transport,
{
    /// Create a new transport dispatcher
    pub fn new(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send a JSON-RPC request and wait for response
    async fn send_request(&self, request: JsonRpcRequest) -> ServerResult<JsonRpcResponse> {
        use turbomcp_transport::{TransportMessage, core::TransportMessageMetadata};

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

        // Serialize request to JSON
        let json = serde_json::to_vec(&request).map_err(|e| {
            McpError::internal(format!("Failed to serialize request: {}", e))
                .with_operation("transport_dispatcher")
        })?;

        // Send via transport
        let transport_msg = TransportMessage::with_metadata(
            MessageId::Uuid(uuid::Uuid::new_v4()),
            bytes::Bytes::from(json),
            TransportMessageMetadata::with_content_type("application/json"),
        );

        self.transport
            .send(transport_msg)
            .await
            .map_err(|e| {
                McpError::internal(format!("Failed to send request via transport: {}", e))
                    .with_operation("transport_dispatcher")
            })?;

        // Wait for response with timeout (60 seconds per MCP recommendation)
        match tokio::time::timeout(tokio::time::Duration::from_secs(60), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                Err(McpError::internal("Response channel closed")
                    .with_operation("transport_dispatcher"))
            }
            Err(_) => {
                // Timeout - remove from pending
                self.pending_requests.lock().await.remove(&request_id);
                Err(McpError::timeout("Request timeout (60s)")
                    .with_operation("transport_dispatcher"))
            }
        }
    }

    /// Generate a unique request ID
    fn generate_request_id() -> MessageId {
        MessageId::String(uuid::Uuid::new_v4().to_string())
    }

    /// Get access to pending requests for response correlation
    pub fn pending_requests(
        &self,
    ) -> Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> {
        Arc::clone(&self.pending_requests)
    }

    /// Get access to the transport
    pub fn transport(&self) -> Arc<T> {
        Arc::clone(&self.transport)
    }
}

#[async_trait::async_trait]
impl<T> ServerRequestDispatcher for TransportDispatcher<T>
where
    T: turbomcp_transport::Transport + Send + Sync + 'static,
{
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "elicitation/create".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| {
                    McpError::internal(format!("Failed to serialize elicitation request: {}", e))
                        .with_operation("MCP compliance")
                })?,
            ),
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| {
                McpError::internal(format!("Invalid elicitation response format: {}", e))
                    .with_operation("MCP compliance")
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal(
                    "Invalid elicitation response: missing result and error",
                )
                .with_operation("MCP compliance"),
            )
        }
    }

    async fn send_ping(
        &self,
        _request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "ping".to_string(),
            params: None,
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if response.result().is_some() {
            Ok(PingResult {
                data: None,
                _meta: None,
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal("Invalid ping response")
                    .with_operation("MCP compliance"),
            )
        }
    }

    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "sampling/createMessage".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| {
                    McpError::internal(format!("Failed to serialize sampling request: {}", e))
                        .with_operation("MCP compliance")
                })?,
            ),
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| {
                McpError::internal(format!("Invalid sampling response format: {}", e))
                    .with_operation("MCP compliance")
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal("Invalid sampling response: missing result and error")
                    .with_operation("MCP compliance"),
            )
        }
    }

    async fn send_list_roots(
        &self,
        _request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "roots/list".to_string(),
            params: None,
            id: Self::generate_request_id(),
        };

        let response = self.send_request(json_rpc_request).await?;

        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| {
                McpError::internal(format!("Invalid roots response format: {}", e))
                    .with_operation("MCP compliance")
            })
        } else if let Some(error) = response.error() {
            // Preserve client error code by wrapping as Protocol error
            Err(McpError::from_rpc_code(error.code, &error.message))
        } else {
            Err(
                McpError::internal("Invalid roots response: missing result and error")
                    .with_operation("MCP compliance"),
            )
        }
    }

    fn supports_bidirectional(&self) -> bool {
        self.transport.capabilities().supports_bidirectional
    }

    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>> {
        Ok(None)
    }
}

/// Run MCP server with any Transport implementation with full bidirectional support
///
/// This is a generic runtime that works with TCP, Unix Socket, or any other
/// transport implementing the `Transport` trait.
///
/// **Architecture**:
/// - Spawns receiver task: reads from transport, routes through router
/// - Transport send: used for both responses and server-initiated requests
/// - Correlation: matches responses to pending requests
///
/// **Usage**:
/// ```rust,ignore
/// use std::sync::Arc;
/// use turbomcp_transport::TcpTransport;
/// use turbomcp_server::runtime::{TransportDispatcher, run_transport_bidirectional};
/// use turbomcp_server::routing::RequestRouter;
///
/// let addr = "127.0.0.1:8080".parse().unwrap();
/// let transport = TcpTransport::new_server(addr);
/// let dispatcher = TransportDispatcher::new(transport);
/// let router = Arc::new(RequestRouter::new());
///
/// // In async context:
/// run_transport_bidirectional(router, dispatcher).await?;
/// ```
pub async fn run_transport_bidirectional<T>(
    router: Arc<RequestRouter>,
    dispatcher: TransportDispatcher<T>,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: turbomcp_transport::Transport + Send + Sync + 'static,
{
    let transport = dispatcher.transport();
    let pending_requests = dispatcher.pending_requests();

    // Main message processing loop
    loop {
        // Receive message from transport
        match transport.receive().await {
            Ok(Some(message)) => {
                // Try parsing as JSON-RPC response first (for server-initiated request responses)
                if let Ok(response) = serde_json::from_slice::<JsonRpcResponse>(&message.payload) {
                    let request_id = match &response.id {
                        turbomcp_protocol::jsonrpc::ResponseId(Some(id)) => match id {
                            MessageId::String(s) => s.clone(),
                            MessageId::Number(n) => n.to_string(),
                            MessageId::Uuid(u) => u.to_string(),
                        },
                        _ => continue,
                    };

                    if let Some(tx) = pending_requests.lock().await.remove(&request_id) {
                        let _ = tx.send(response);
                    }
                    continue;
                }

                // Try parsing as JSON-RPC request (client-initiated)
                if let Ok(request) = serde_json::from_slice::<JsonRpcRequest>(&message.payload) {
                    let router = Arc::clone(&router);
                    let transport = Arc::clone(&transport);

                    tokio::spawn(async move {
                        // Create properly configured context with server-to-client capabilities
                        let ctx = router.create_context(None, None, None);
                        let response = router.route(request, ctx).await;

                        // Send response back via transport
                        if let Ok(json) = serde_json::to_vec(&response) {
                            use turbomcp_transport::{
                                TransportMessage, core::TransportMessageMetadata,
                            };
                            let transport_msg = TransportMessage::with_metadata(
                                MessageId::Uuid(uuid::Uuid::new_v4()),
                                bytes::Bytes::from(json),
                                TransportMessageMetadata::with_content_type("application/json"),
                            );
                            let _ = transport.send(transport_msg).await;
                        }
                    });
                }
            }
            Ok(None) => {
                // No message available, sleep briefly
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Transport receive failed");
                break;
            }
        }
    }

    Ok(())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stdio_dispatcher_clean_shutdown() {
        // Test that STDIO dispatcher can be dropped without panic
        let (tx, _rx) = mpsc::unbounded_channel();
        let dispatcher = StdioDispatcher::new(tx);

        // Should not panic on drop
        drop(dispatcher);
    }

    #[tokio::test]
    async fn test_stdio_dispatcher_creation() {
        // Test that StdioDispatcher can be created and used
        let (tx, _rx) = mpsc::unbounded_channel();
        let dispatcher = StdioDispatcher::new(tx.clone());

        // Clone should work (needed for concurrent usage)
        let _dispatcher2 = dispatcher.clone();

        // Sending messages should work
        assert!(tx.send(StdioMessage::Shutdown).is_ok());
    }

    #[tokio::test]
    async fn test_joinset_task_tracking() {
        // Test that JoinSet properly tracks and cleans up tasks
        let mut tasks = JoinSet::new();

        // Spawn some test tasks
        for i in 0..5 {
            tasks.spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(i * 10)).await;
            });
        }

        assert_eq!(tasks.len(), 5);

        // Wait for all tasks to complete
        let mut completed = 0;
        while let Some(result) = tasks.join_next().await {
            assert!(result.is_ok());
            completed += 1;
        }

        assert_eq!(completed, 5);
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn test_joinset_with_timeout() {
        // Test timeout behavior for slow tasks
        let mut tasks = JoinSet::new();

        // Spawn a slow task
        tasks.spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        });

        // Wait with short timeout
        let timeout = std::time::Duration::from_millis(100);
        let start = std::time::Instant::now();

        let result = tokio::time::timeout(timeout, tasks.join_next()).await;

        // Should timeout
        assert!(result.is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));

        // Cleanup
        tasks.shutdown().await;
    }

    #[tokio::test]
    async fn test_stdio_message_types() {
        // Test that StdioMessage enum works correctly
        use turbomcp_protocol::jsonrpc::JsonRpcRequest;

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "test".to_string(),
            params: None,
            id: MessageId::String("test-1".to_string()),
        };

        let msg = StdioMessage::ServerRequest { request };

        match msg {
            StdioMessage::ServerRequest { .. } => { /* OK */ }
            _ => panic!("Expected ServerRequest"),
        }

        let shutdown_msg = StdioMessage::Shutdown;
        match shutdown_msg {
            StdioMessage::Shutdown => { /* OK */ }
            _ => panic!("Expected Shutdown"),
        }
    }

    #[tokio::test]
    async fn test_pending_requests_cleanup() {
        // Test that pending requests are properly cleaned up
        let pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (tx, _rx) = oneshot::channel();
        pending_requests
            .lock()
            .await
            .insert("test-id".to_string(), tx);

        assert_eq!(pending_requests.lock().await.len(), 1);

        // Remove the request
        let removed = pending_requests.lock().await.remove("test-id");
        assert!(removed.is_some());
        assert_eq!(pending_requests.lock().await.len(), 0);
    }
}
