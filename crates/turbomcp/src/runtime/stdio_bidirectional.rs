//! STDIO Runtime - Full MCP 2025-06-18 Protocol over stdin/stdout
//!
//! **Status**: Production implementation following MCP 2025-06-18 spec
//!
//! This module implements the complete MCP protocol over stdio transport, supporting
//! both client→server (tools, resources, prompts) and server→client (sampling,
//! elicitation, roots, ping) requests with concurrent handling.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐
//! │ stdin reader │ ──┐
//! └──────────────┘   │
//!                    ├─► Message Router
//! ┌───────────────┐  │      │
//! │ stdout writer │◄─┘      │
//! └───────────────┘          │
//!                            ├─► Client Request → Server Handler
//!                            │
//!                            └─► Client Response → Pending Request
//! ```

use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc, oneshot};

use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult, PingRequest, PingResult,
};
use turbomcp_protocol::{JsonRpcHandler, RequestContext};
use turbomcp_server::routing::ServerRequestDispatcher;

use crate::{MessageId, ServerError, ServerResult};

/// STDIO dispatcher for server-initiated requests
///
/// This dispatcher implements the MCP 2025-06-18 specification for stdio transport,
/// allowing servers to make requests to clients (server→client capability) while
/// maintaining proper request/response correlation.
///
/// ## MCP Compliance
///
/// - Sends JSON-RPC 2.0 formatted requests to stdout
/// - Generates unique request IDs for correlation
/// - Handles responses asynchronously
/// - Supports: sampling/createMessage, elicitation/create, roots/list, ping
#[derive(Clone)]
pub struct StdioDispatcher {
    /// Channel for sending messages to stdout writer
    request_tx: mpsc::UnboundedSender<StdioMessage>,
    /// Pending server-initiated requests awaiting responses
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
}

/// Internal message type for STDIO transport
///
/// Messages flow through the stdout writer task which serializes them to JSON
/// and writes to stdout per MCP spec.
pub enum StdioMessage {
    /// Server request to be sent to client
    ServerRequest {
        /// The JSON-RPC request (MCP 2025-06-18 compliant)
        request: JsonRpcRequest,
    },
    /// Shutdown signal
    Shutdown,
}

impl StdioDispatcher {
    /// Create a new STDIO dispatcher
    ///
    /// # Arguments
    ///
    /// * `request_tx` - Channel to stdout writer for sending JSON-RPC requests
    pub fn new(request_tx: mpsc::UnboundedSender<StdioMessage>) -> Self {
        Self {
            request_tx,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send a JSON-RPC request and wait for response
    ///
    /// This is the core method that:
    /// 1. Registers the request as pending
    /// 2. Sends to stdout via channel
    /// 3. Waits for correlated response from stdin
    ///
    /// ## MCP 2025-06-18 Compliance
    ///
    /// - Uses JSON-RPC 2.0 format
    /// - Generates unique request IDs (UUID v4)
    /// - Handles errors per MCP error codes
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
            .map_err(|e| ServerError::Handler {
                message: format!("Failed to send request to stdout: {}", e),
                context: Some("stdio_dispatcher".to_string()),
            })?;

        // Wait for response with timeout (60 seconds per MCP recommendation)
        match tokio::time::timeout(tokio::time::Duration::from_secs(60), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(ServerError::Handler {
                message: "Response channel closed".to_string(),
                context: Some("stdio_dispatcher".to_string()),
            }),
            Err(_) => {
                // Timeout - remove from pending
                self.pending_requests.lock().await.remove(&request_id);
                Err(ServerError::Handler {
                    message: "Request timeout (60s)".to_string(),
                    context: Some("stdio_dispatcher".to_string()),
                })
            }
        }
    }

    /// Create a unique request ID (UUID v4 per MCP best practices)
    fn generate_request_id() -> MessageId {
        MessageId::String(uuid::Uuid::new_v4().to_string())
    }
}

#[async_trait::async_trait]
impl ServerRequestDispatcher for StdioDispatcher {
    /// Send an elicitation request to the client
    ///
    /// ## MCP 2025-06-18 Spec: elicitation/create
    ///
    /// Request format:
    /// ```json
    /// {
    ///   "jsonrpc": "2.0",
    ///   "id": "uuid",
    ///   "method": "elicitation/create",
    ///   "params": {
    ///     "message": "Please provide input",
    ///     "requestedSchema": { "type": "object", ... }
    ///   }
    /// }
    /// ```
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        // Create MCP-compliant JSON-RPC request
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "elicitation/create".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| ServerError::Handler {
                    message: format!("Failed to serialize elicitation request: {}", e),
                    context: Some("MCP compliance".to_string()),
                })?,
            ),
            id: Self::generate_request_id(),
        };

        // Send and await response
        let response = self.send_request(json_rpc_request).await?;

        // Parse MCP-compliant response
        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| ServerError::Handler {
                message: format!("Invalid elicitation response format: {}", e),
                context: Some("MCP compliance".to_string()),
            })
        } else if let Some(error) = response.error() {
            // FIXED: Preserve error code by creating Protocol error, not Handler error
            let protocol_err = turbomcp_protocol::Error::rpc(error.code, &error.message);
            Err(ServerError::Protocol(protocol_err))
        } else {
            Err(ServerError::Handler {
                message: "Invalid elicitation response: missing result and error".to_string(),
                context: Some("MCP compliance".to_string()),
            })
        }
    }

    /// Send a ping request to the client
    ///
    /// ## MCP 2025-06-18 Spec: ping
    ///
    /// Request format:
    /// ```json
    /// {
    ///   "jsonrpc": "2.0",
    ///   "id": "uuid",
    ///   "method": "ping"
    /// }
    /// ```
    async fn send_ping(
        &self,
        _request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        // Create MCP-compliant JSON-RPC ping request
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "ping".to_string(),
            params: None, // Ping has no params per MCP spec
            id: Self::generate_request_id(),
        };

        // Send and await response
        let response = self.send_request(json_rpc_request).await?;

        // Ping response is empty {} per MCP spec
        if response.result().is_some() {
            Ok(PingResult {
                data: None,
                _meta: None,
            })
        } else if let Some(error) = response.error() {
            // FIXED: Preserve error code by creating Protocol error, not Handler error
            let protocol_err = turbomcp_protocol::Error::rpc(error.code, &error.message);
            Err(ServerError::Protocol(protocol_err))
        } else {
            Err(ServerError::Handler {
                message: "Invalid ping response".to_string(),
                context: Some("MCP compliance".to_string()),
            })
        }
    }

    /// Send a sampling/createMessage request to the client
    ///
    /// ## MCP 2025-06-18 Spec: sampling/createMessage
    ///
    /// Request format:
    /// ```json
    /// {
    ///   "jsonrpc": "2.0",
    ///   "id": "uuid",
    ///   "method": "sampling/createMessage",
    ///   "params": {
    ///     "messages": [...],
    ///     "modelPreferences": {...},
    ///     "systemPrompt": "...",
    ///     "maxTokens": 100
    ///   }
    /// }
    /// ```
    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        // Create MCP-compliant JSON-RPC request
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "sampling/createMessage".to_string(),
            params: Some(
                serde_json::to_value(&request).map_err(|e| ServerError::Handler {
                    message: format!("Failed to serialize sampling request: {}", e),
                    context: Some("MCP compliance".to_string()),
                })?,
            ),
            id: Self::generate_request_id(),
        };

        // Send and await response
        let response = self.send_request(json_rpc_request).await?;

        // Parse MCP-compliant response
        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| ServerError::Handler {
                message: format!("Invalid sampling response format: {}", e),
                context: Some("MCP compliance".to_string()),
            })
        } else if let Some(error) = response.error() {
            // FIXED: Preserve error code by creating Protocol error, not Handler error
            // This ensures user rejection (code -1) is not wrapped as -32002
            let protocol_err = turbomcp_protocol::Error::rpc(error.code, &error.message);
            Err(ServerError::Protocol(protocol_err))
        } else {
            Err(ServerError::Handler {
                message: "Invalid sampling response: missing result and error".to_string(),
                context: Some("MCP compliance".to_string()),
            })
        }
    }

    /// Send a roots/list request to the client
    ///
    /// ## MCP 2025-06-18 Spec: roots/list
    ///
    /// Request format:
    /// ```json
    /// {
    ///   "jsonrpc": "2.0",
    ///   "id": "uuid",
    ///   "method": "roots/list"
    /// }
    /// ```
    async fn send_list_roots(
        &self,
        _request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        // Create MCP-compliant JSON-RPC request
        let json_rpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "roots/list".to_string(),
            params: None, // roots/list has no params per MCP spec
            id: Self::generate_request_id(),
        };

        // Send and await response
        let response = self.send_request(json_rpc_request).await?;

        // Parse MCP-compliant response
        if let Some(result) = response.result() {
            serde_json::from_value(result.clone()).map_err(|e| ServerError::Handler {
                message: format!("Invalid roots response format: {}", e),
                context: Some("MCP compliance".to_string()),
            })
        } else if let Some(error) = response.error() {
            // FIXED: Preserve error code by creating Protocol error, not Handler error
            let protocol_err = turbomcp_protocol::Error::rpc(error.code, &error.message);
            Err(ServerError::Protocol(protocol_err))
        } else {
            Err(ServerError::Handler {
                message: "Invalid roots response: missing result and error".to_string(),
                context: Some("MCP compliance".to_string()),
            })
        }
    }

    /// Check if bidirectional communication is supported
    ///
    /// For STDIO transport, bidirectional is always supported when dispatcher is configured.
    fn supports_bidirectional(&self) -> bool {
        true
    }

    /// Get client capabilities
    ///
    /// Returns None for STDIO as capabilities are exchanged during initialize.
    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>> {
        Ok(None)
    }
}

/// Run MCP server over STDIO transport
///
/// This function implements the complete MCP 2025-06-18 stdio protocol:
/// - Reads JSON-RPC from stdin (client requests AND server response correlations)
/// - Writes JSON-RPC to stdout (server responses AND server requests)
/// - Maintains request/response correlation
/// - Handles errors per MCP spec
///
/// ## MCP Compliance
///
/// - stdout is EXCLUSIVELY for JSON-RPC messages (no debug output)
/// - stdin/stdout use line-delimited JSON (one message per line)
/// - Proper JSON-RPC 2.0 format for all messages
/// - Request IDs must be preserved in responses
///
/// ## Architecture
///
/// Two concurrent tasks:
/// 1. **stdout writer**: Sends server-initiated requests to client
/// 2. **stdin reader**: Handles client requests and correlates responses
pub async fn run_stdio<H>(
    handler: H,
    dispatcher: StdioDispatcher,
    mut request_rx: mpsc::UnboundedReceiver<StdioMessage>,
) -> Result<(), Box<dyn StdError>>
where
    H: Send + Sync + 'static,
    H: JsonRpcHandler,
{
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    let handler = Arc::new(handler);
    let stdout = Arc::new(Mutex::new(stdout));
    let pending_requests = Arc::clone(&dispatcher.pending_requests);

    // Spawn stdout writer task
    let stdout_writer = Arc::clone(&stdout);
    tokio::spawn(async move {
        while let Some(msg) = request_rx.recv().await {
            match msg {
                StdioMessage::ServerRequest { request } => {
                    // Serialize to JSON-RPC per MCP spec
                    if let Ok(json) = serde_json::to_string(&request) {
                        let mut stdout = stdout_writer.lock().await;
                        // Write line-delimited JSON per MCP spec
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
                    // This is a response to a server-initiated request
                    let request_id = match &response.id {
                        turbomcp_protocol::jsonrpc::ResponseId(Some(id)) => match id {
                            MessageId::String(s) => s.clone(),
                            MessageId::Number(n) => n.to_string(),
                            MessageId::Uuid(u) => u.to_string(),
                        },
                        _ => continue,
                    };

                    // Correlate with pending request
                    if let Some(tx) = pending_requests.lock().await.remove(&request_id) {
                        let _ = tx.send(response);
                    }
                    continue;
                }

                // Try parsing as JSON-RPC request (client-initiated)
                if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&line) {
                    // This is a client request - handle it
                    let handler = Arc::clone(&handler);
                    let stdout = Arc::clone(&stdout);

                    tokio::spawn(async move {
                        // Handle via bidirectional wrapper
                        let response_value = handler
                            .handle_request(serde_json::to_value(&request).unwrap_or_default())
                            .await;

                        // Write response to stdout
                        if let Ok(json) = serde_json::to_string(&response_value) {
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

    Ok(())
}
