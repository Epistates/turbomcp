//! Bidirectional WebSocket support for MCP transport layer
//!
//! This module provides full MCP 2025-06-18 bidirectional communication support
//! over WebSocket, enabling server→client requests (sampling, elicitation, ping, roots).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::Message as WsMessage;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use turbomcp_protocol::{
    MessageId,
    jsonrpc::{
        JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload, JsonRpcVersion, ResponseId,
    },
    types::{
        CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
        ListRootsResult, PingRequest, PingResult,
    },
};

/// Pending server-initiated requests for response correlation
type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>;

/// WebSocket dispatcher for server-initiated requests
///
/// This dispatcher enables full bidirectional MCP communication over WebSocket,
/// allowing the server to send requests to the client and await responses.
///
/// ## Architecture
///
/// - Uses mpsc channel to send messages to WebSocket send loop
/// - Maintains HashMap of pending requests for response correlation
/// - Implements timeout and error handling for server→client requests
#[derive(Clone, Debug)]
pub struct WebSocketDispatcher {
    /// Channel to send messages to the WebSocket send loop
    sender: mpsc::UnboundedSender<WsMessage>,
    /// Pending server-initiated requests
    pending_requests: PendingRequests,
}

impl WebSocketDispatcher {
    /// Create a new WebSocket dispatcher
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
    /// ## Implementation Details
    ///
    /// 1. Generates unique UUID request ID
    /// 2. Sends JSON-RPC 2.0 formatted request
    /// 3. Registers pending request for correlation
    /// 4. Awaits response with 60-second timeout
    async fn send_request<Req, Res>(&self, method: &str, params: Req) -> Result<Res, String>
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
                serde_json::to_value(params)
                    .map_err(|e| format!("Failed to serialize request params: {}", e))?,
            ),
        };

        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();

        // Register pending request
        self.pending_requests
            .lock()
            .await
            .insert(request_id.clone(), response_tx);

        tracing::debug!(
            "📤 Registered pending request: method={}, id={}",
            method,
            request_id
        );

        // Serialize and send request
        let request_json = serde_json::to_string(&request)
            .map_err(|e| format!("Failed to serialize JSON-RPC request: {}", e))?;

        tracing::debug!("📤 Sending server-initiated request: {}", request_json);

        self.sender
            .send(WsMessage::Text(request_json.into()))
            .map_err(|e| {
                // Clean up pending request since send failed
                let pending = self.pending_requests.clone();
                let req_id = request_id.clone();
                tokio::spawn(async move {
                    pending.lock().await.remove(&req_id);
                });
                format!("Failed to send WebSocket message: {}", e)
            })?;

        tracing::debug!("⏳ Awaiting response for request id={}", request_id);

        // Await response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(60), response_rx)
            .await
            .map_err(|_| {
                tracing::error!(
                    "⏱️ Timeout after 60s waiting for response to request id={}",
                    request_id
                );
                // Clean up pending request on timeout
                let pending = self.pending_requests.clone();
                let req_id = request_id.clone();
                tokio::spawn(async move {
                    pending.lock().await.remove(&req_id);
                });
                format!("Request timeout (60s): method={}", method)
            })?
            .map_err(|_| {
                tracing::error!("📪 Response channel closed for request id={}", request_id);
                format!("Response channel closed: method={}", method)
            })?;

        tracing::debug!("✅ Received response for request id={}", request_id);

        // Parse response
        match response.payload {
            JsonRpcResponsePayload::Success { result } => serde_json::from_value(result)
                .map_err(|e| format!("Failed to deserialize response: {}", e)),
            JsonRpcResponsePayload::Error { error } => Err(format!(
                "Request failed: {} (code: {})",
                error.message, error.code
            )),
        }
    }
}

// Note: We cannot implement turbomcp_server::routing::ServerRequestDispatcher here
// because that would create a circular dependency (transport → server → transport).
//
// Instead, we provide this dispatcher as infrastructure, and turbomcp-server
// will provide an adapter that wraps this dispatcher and implements ServerRequestDispatcher.
//
// For now, we provide the core send_request functionality that can be wrapped.

impl WebSocketDispatcher {
    /// Send elicitation request (will be wrapped by adapter in turbomcp-server)
    pub async fn send_elicitation_request(
        &self,
        request: ElicitRequest,
    ) -> Result<ElicitResult, String> {
        self.send_request("elicitation/create", request).await
    }

    /// Send ping request (will be wrapped by adapter in turbomcp-server)
    pub async fn send_ping_request(&self, request: PingRequest) -> Result<PingResult, String> {
        self.send_request("ping", request).await
    }

    /// Send create message request (will be wrapped by adapter in turbomcp-server)
    pub async fn send_create_message_request(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, String> {
        self.send_request("sampling/createMessage", request).await
    }

    /// Send list roots request (will be wrapped by adapter in turbomcp-server)
    pub async fn send_list_roots_request(
        &self,
        request: ListRootsRequest,
    ) -> Result<ListRootsResult, String> {
        self.send_request("roots/list", request).await
    }

    /// Check if bidirectional communication is supported
    pub fn supports_bidirectional(&self) -> bool {
        true
    }
}

/// Handle response correlation for server-initiated requests
///
/// This function should be called from the receive loop when a JsonRpcResponse is received.
/// It checks if the response matches a pending server-initiated request and sends it
/// to the waiting task via the oneshot channel.
///
/// Returns true if the response was matched to a pending request, false otherwise.
pub async fn handle_response_correlation(
    response: JsonRpcResponse,
    pending_requests: &PendingRequests,
) -> bool {
    let response_id = match &response.id {
        ResponseId(Some(id)) => match id {
            MessageId::String(s) => s.clone(),
            MessageId::Number(n) => n.to_string(),
            MessageId::Uuid(u) => u.to_string(),
        },
        _ => {
            tracing::warn!("Received JSON-RPC response with null ID, ignoring");
            return false;
        }
    };

    tracing::debug!("🔍 Checking response correlation: id={}", response_id);

    // Check if we have a pending request with this ID
    if let Some(tx) = pending_requests.lock().await.remove(&response_id) {
        tracing::debug!("✅ Matched pending request: id={}", response_id);
        let _ = tx.send(response);
        true
    } else {
        tracing::warn!(
            "❌ Received response for unknown/expired request: id={}",
            response_id
        );
        false
    }
}

/// Check if a message is a response (vs a request)
pub fn is_response(value: &Value) -> bool {
    value.get("result").is_some() || value.get("error").is_some()
}
