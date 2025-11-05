//! Protocol client for JSON-RPC communication
//!
//! This module provides the ProtocolClient which handles the low-level
//! JSON-RPC protocol communication with MCP servers.
//!
//! ## Bidirectional Communication Architecture
//!
//! The ProtocolClient uses a MessageDispatcher to solve the bidirectional
//! communication problem. Instead of directly calling `transport.receive()`,
//! which created race conditions when multiple code paths tried to receive,
//! we now use a centralized message routing layer:
//!
//! ```text
//! ProtocolClient::request()
//!     ↓
//!   1. Register oneshot channel with dispatcher
//!   2. Send request via transport
//!   3. Wait on oneshot channel
//!     ↓
//! MessageDispatcher (background task)
//!     ↓
//!   Continuously reads transport.receive()
//!   Routes responses → oneshot channels
//!   Routes requests → Client handlers
//! ```
//!
//! This ensures there's only ONE consumer of transport.receive(),
//! eliminating the race condition.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};
use turbomcp_protocol::{Error, Result};
use turbomcp_transport::{Transport, TransportConfig, TransportMessage};

use super::dispatcher::MessageDispatcher;

/// JSON-RPC protocol handler for MCP communication
///
/// Handles request/response correlation, serialization, and protocol-level concerns.
/// This is the abstraction layer between raw Transport and high-level Client APIs.
///
/// ## Architecture
///
/// The ProtocolClient now uses a MessageDispatcher to handle bidirectional
/// communication correctly. The dispatcher runs a background task that:
/// - Reads ALL messages from the transport
/// - Routes responses to waiting request() calls
/// - Routes incoming requests to registered handlers
///
/// This eliminates race conditions by centralizing all message routing
/// in a single background task.
#[derive(Debug)]
pub(super) struct ProtocolClient<T: Transport> {
    transport: Arc<T>,
    dispatcher: Arc<MessageDispatcher>,
    next_id: AtomicU64,
    /// Transport configuration for timeout enforcement (v2.2.0+)
    config: TransportConfig,
}

impl<T: Transport + 'static> ProtocolClient<T> {
    /// Create a new protocol client with message dispatcher
    ///
    /// This automatically starts the message routing background task.
    pub(super) fn new(transport: T) -> Self {
        let transport = Arc::new(transport);
        let dispatcher = MessageDispatcher::new(transport.clone());

        Self {
            transport,
            dispatcher,
            next_id: AtomicU64::new(1),
            config: TransportConfig::default(), // Use default timeout config
        }
    }

    /// Create a new protocol client with custom transport configuration
    ///
    /// This allows setting custom timeouts and limits.
    #[allow(dead_code)] // May be used in future
    pub(super) fn with_config(transport: T, config: TransportConfig) -> Self {
        let transport = Arc::new(transport);
        let dispatcher = MessageDispatcher::new(transport.clone());

        Self {
            transport,
            dispatcher,
            next_id: AtomicU64::new(1),
            config,
        }
    }

    /// Get the message dispatcher for handler registration
    ///
    /// This allows the Client to register request/notification handlers
    /// with the dispatcher.
    pub(super) fn dispatcher(&self) -> &Arc<MessageDispatcher> {
        &self.dispatcher
    }

    /// Send JSON-RPC request and await typed response
    ///
    /// ## New Architecture (v2.0+)
    ///
    /// Instead of calling `transport.receive()` directly (which created the
    /// race condition), this method now:
    ///
    /// 1. Registers a oneshot channel with the dispatcher BEFORE sending
    /// 2. Sends the request via transport
    /// 3. Waits on the oneshot channel for the response
    ///
    /// The dispatcher's background task receives the response and routes it
    /// to the oneshot channel. This ensures responses always reach the right
    /// request() call, even when the server sends requests (elicitation, etc.)
    /// in between.
    ///
    /// ## Example Flow with Elicitation
    ///
    /// ```text
    /// Client: call_tool("test") → request(id=1)
    ///   1. Register oneshot channel for id=1
    ///   2. Send tools/call request
    ///   3. Wait on channel...
    ///
    /// Server: Sends elicitation/create request (id=2)
    ///   → Dispatcher routes to request handler
    ///   → Client processes elicitation
    ///   → Client sends elicitation response
    ///
    /// Server: Sends tools/call response (id=1)
    ///   → Dispatcher routes to oneshot channel for id=1
    ///   → request() receives response ✓
    /// ```
    pub(super) async fn request<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        // Wrap the entire operation in total timeout (if configured)
        let operation = self.request_inner(method, params);

        if let Some(total_timeout) = self.config.timeouts.total {
            match tokio::time::timeout(total_timeout, operation).await {
                Ok(result) => result,
                Err(_) => {
                    let err = turbomcp_transport::TransportError::TotalTimeout {
                        operation: format!("{}()", method),
                        timeout: total_timeout,
                    };
                    Err(Error::transport(err.to_string()))
                }
            }
        } else {
            operation.await
        }
    }

    /// Inner request implementation without total timeout wrapper
    async fn request_inner<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        // Generate unique request ID
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request_id = turbomcp_protocol::MessageId::from(id.to_string());

        // Build JSON-RPC request
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: request_id.clone(),
            method: method.to_string(),
            params,
        };

        // Step 1: Register oneshot channel BEFORE sending request
        // This ensures the dispatcher can route the response when it arrives
        let response_receiver = self.dispatcher.wait_for_response(request_id.clone());

        // Step 2: Serialize and send request
        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::protocol(format!("Failed to serialize request: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from(format!("req-{id}")),
            payload.into(),
        );

        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))?;

        // Step 3: Wait for response via oneshot channel with request timeout
        // The dispatcher's background task will send the response when it arrives
        let response = if let Some(request_timeout) = self.config.timeouts.request {
            match tokio::time::timeout(request_timeout, response_receiver).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => return Err(Error::transport("Response channel closed".to_string())),
                Err(_) => {
                    let err = turbomcp_transport::TransportError::RequestTimeout {
                        operation: format!("{}()", method),
                        timeout: request_timeout,
                    };
                    return Err(Error::transport(err.to_string()));
                }
            }
        } else {
            response_receiver
                .await
                .map_err(|_| Error::transport("Response channel closed".to_string()))?
        };

        // Handle JSON-RPC errors
        if let Some(error) = response.error() {
            tracing::info!(
                "🔍 [protocol.rs] Received JSON-RPC error - code: {}, message: {}",
                error.code,
                error.message
            );
            let err = Error::rpc(error.code, &error.message);
            tracing::info!(
                "🔍 [protocol.rs] Created Error - kind: {:?}, jsonrpc_code: {}",
                err.kind,
                err.jsonrpc_error_code()
            );
            return Err(err);
        }

        // Deserialize result
        serde_json::from_value(response.result().unwrap_or_default().clone())
            .map_err(|e| Error::protocol(format!("Failed to deserialize response: {e}")))
    }

    /// Send JSON-RPC notification (no response expected)
    pub(super) async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::protocol(format!("Failed to serialize notification: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from("notification"),
            payload.into(),
        );

        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))
    }

    /// Connect the transport
    #[allow(dead_code)] // Reserved for future use
    pub(super) async fn connect(&self) -> Result<()> {
        self.transport
            .connect()
            .await
            .map_err(|e| Error::transport(format!("Transport connect failed: {e}")))
    }

    /// Disconnect the transport
    #[allow(dead_code)] // Reserved for future use
    pub(super) async fn disconnect(&self) -> Result<()> {
        self.transport
            .disconnect()
            .await
            .map_err(|e| Error::transport(format!("Transport disconnect failed: {e}")))
    }

    /// Get transport reference
    ///
    /// Returns an Arc reference to the transport, allowing it to be shared
    /// with other components (like the message dispatcher).
    pub(super) fn transport(&self) -> &Arc<T> {
        &self.transport
    }
}
