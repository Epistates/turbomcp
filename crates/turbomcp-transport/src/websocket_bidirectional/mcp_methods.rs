//! MCP 2025-06-18 bidirectional methods for WebSocket transport
//!
//! This module implements server-initiated MCP methods (ping, sampling, roots)
//! for the WebSocket bidirectional transport, following the MCP 2025-06-18 specification.
//!
//! These methods enable servers to make requests to clients:
//! - `ping` - Connection health checks
//! - `sampling/createMessage` - LLM sampling requests
//! - `roots/list` - Client filesystem roots listing
//!
//! ## Architecture
//!
//! Each method follows the same pattern as `send_elicitation`:
//! 1. Generate unique request ID (UUID)
//! 2. Create oneshot channel for response
//! 3. Store pending request in correlations map
//! 4. Send JSON-RPC request via WebSocket
//! 5. Wait for correlated response with timeout
//! 6. Parse and return result or error
//!
//! ## MCP Compliance
//!
//! All methods follow MCP 2025-06-18 specification:
//! - Correct method names (`ping`, `sampling/createMessage`, `roots/list`)
//! - JSON-RPC 2.0 format
//! - UUID request IDs for correlation
//! - 60-second default timeout
//! - Proper error handling

use std::time::Duration;

use futures::SinkExt as _;
use serde_json::json;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, warn};
use uuid::Uuid;

use super::types::WebSocketBidirectionalTransport;
use crate::core::{TransportError, TransportResult};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ListRootsRequest, ListRootsResult, PingRequest,
    PingResult,
};

/// Pending MCP request (generic for ping, sampling, roots)
///
/// NOTE: This struct is reserved for future correlation map implementation.
/// Currently unused but kept for forward compatibility.
#[allow(dead_code)]
struct PendingMcpRequest<T> {
    request_id: String,
    response_tx: oneshot::Sender<T>,
    deadline: tokio::time::Instant,
}

#[allow(dead_code)]
impl<T> PendingMcpRequest<T> {
    fn new(response_tx: oneshot::Sender<T>, timeout: Duration) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            response_tx,
            deadline: tokio::time::Instant::now() + timeout,
        }
    }
}

impl WebSocketBidirectionalTransport {
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
    ///
    /// # Arguments
    ///
    /// * `_request` - Ping request (currently no parameters per MCP spec)
    /// * `timeout_duration` - Optional timeout (default: 60 seconds per MCP spec)
    ///
    /// # Returns
    ///
    /// * `Ok(PingResult)` - Ping response (empty per MCP spec)
    /// * `Err(TransportError)` - Connection error, timeout, or parse error
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_transport::websocket_bidirectional::WebSocketBidirectionalTransport;
    /// # use turbomcp_protocol::types::{PingRequest, PingResult};
    /// # async fn example(transport: &WebSocketBidirectionalTransport) {
    /// let request = PingRequest { _meta: None };
    /// let result = transport.send_ping(request, None).await.unwrap();
    /// # }
    /// ```
    pub async fn send_ping(
        &self,
        _request: PingRequest,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<PingResult> {
        let request_id = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = oneshot::channel();

        let timeout_duration = timeout_duration.unwrap_or(Duration::from_secs(60));

        // Create JSON-RPC request per MCP 2025-06-18
        let json_request = json!({
            "jsonrpc": "2.0",
            "method": "ping",
            "id": request_id
        });

        // Store pending request for response matching
        self.pending_pings.insert(request_id.clone(), response_tx);

        // Send via WebSocket
        let message_text = serde_json::to_string(&json_request).map_err(|e| {
            self.pending_pings.remove(&request_id);
            TransportError::SendFailed(format!("Failed to serialize: {}", e))
        })?;

        if let Some(ref mut writer) = *self.writer.lock().await {
            writer
                .send(Message::Text(message_text.into()))
                .await
                .map_err(|e| {
                    self.pending_pings.remove(&request_id);
                    TransportError::SendFailed(format!("WebSocket send failed: {}", e))
                })?;

            debug!(
                "Sent ping request {} for session {}",
                request_id, self.session_id
            );
        } else {
            self.pending_pings.remove(&request_id);
            return Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ));
        }

        // Update metrics
        self.metrics.write().await.messages_sent += 1;

        // Wait for response with timeout
        match timeout(timeout_duration, response_rx).await {
            Ok(Ok(result)) => {
                debug!(
                    "Received ping response for {} in session {}",
                    request_id, self.session_id
                );
                Ok(result)
            }
            Ok(Err(_)) => {
                self.pending_pings.remove(&request_id);
                warn!(
                    "Ping response channel closed for {} in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::ReceiveFailed(
                    "Response channel closed".to_string(),
                ))
            }
            Err(_) => {
                self.pending_pings.remove(&request_id);
                warn!(
                    "Ping {} timed out in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::Timeout)
            }
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
    ///
    /// # Arguments
    ///
    /// * `request` - Sampling request with messages and preferences
    /// * `timeout_duration` - Optional timeout (default: 60 seconds per MCP spec)
    ///
    /// # Returns
    ///
    /// * `Ok(CreateMessageResult)` - Sampling response with generated message
    /// * `Err(TransportError)` - Connection error, timeout, or parse error
    pub async fn send_sampling(
        &self,
        request: CreateMessageRequest,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<CreateMessageResult> {
        let request_id = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = oneshot::channel();

        let timeout_duration = timeout_duration.unwrap_or(Duration::from_secs(60));

        // Create JSON-RPC request per MCP 2025-06-18
        let json_request = json!({
            "jsonrpc": "2.0",
            "method": "sampling/createMessage",
            "params": request,
            "id": request_id
        });

        // Store pending request for response matching
        self.pending_samplings
            .insert(request_id.clone(), response_tx);

        // Send via WebSocket
        let message_text = serde_json::to_string(&json_request).map_err(|e| {
            self.pending_samplings.remove(&request_id);
            TransportError::SendFailed(format!("Failed to serialize: {}", e))
        })?;

        if let Some(ref mut writer) = *self.writer.lock().await {
            writer
                .send(Message::Text(message_text.into()))
                .await
                .map_err(|e| {
                    self.pending_samplings.remove(&request_id);
                    TransportError::SendFailed(format!("WebSocket send failed: {}", e))
                })?;

            debug!(
                "Sent sampling request {} for session {}",
                request_id, self.session_id
            );
        } else {
            self.pending_samplings.remove(&request_id);
            return Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ));
        }

        // Update metrics
        self.metrics.write().await.messages_sent += 1;

        // Wait for response with timeout
        match timeout(timeout_duration, response_rx).await {
            Ok(Ok(result)) => {
                debug!(
                    "Received sampling response for {} in session {}",
                    request_id, self.session_id
                );
                Ok(result)
            }
            Ok(Err(_)) => {
                self.pending_samplings.remove(&request_id);
                warn!(
                    "Sampling response channel closed for {} in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::ReceiveFailed(
                    "Response channel closed".to_string(),
                ))
            }
            Err(_) => {
                self.pending_samplings.remove(&request_id);
                warn!(
                    "Sampling {} timed out in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::Timeout)
            }
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
    ///
    /// # Arguments
    ///
    /// * `_request` - Roots request (currently no parameters per MCP spec)
    /// * `timeout_duration` - Optional timeout (default: 60 seconds per MCP spec)
    ///
    /// # Returns
    ///
    /// * `Ok(ListRootsResult)` - List of client filesystem roots
    /// * `Err(TransportError)` - Connection error, timeout, or parse error
    pub async fn send_list_roots(
        &self,
        _request: ListRootsRequest,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<ListRootsResult> {
        let request_id = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = oneshot::channel();

        let timeout_duration = timeout_duration.unwrap_or(Duration::from_secs(60));

        // Create JSON-RPC request per MCP 2025-06-18
        let json_request = json!({
            "jsonrpc": "2.0",
            "method": "roots/list",
            "id": request_id
        });

        // Store pending request for response matching
        self.pending_roots.insert(request_id.clone(), response_tx);

        // Send via WebSocket
        let message_text = serde_json::to_string(&json_request).map_err(|e| {
            self.pending_roots.remove(&request_id);
            TransportError::SendFailed(format!("Failed to serialize: {}", e))
        })?;

        if let Some(ref mut writer) = *self.writer.lock().await {
            writer
                .send(Message::Text(message_text.into()))
                .await
                .map_err(|e| {
                    self.pending_roots.remove(&request_id);
                    TransportError::SendFailed(format!("WebSocket send failed: {}", e))
                })?;

            debug!(
                "Sent roots/list request {} for session {}",
                request_id, self.session_id
            );
        } else {
            self.pending_roots.remove(&request_id);
            return Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ));
        }

        // Update metrics
        self.metrics.write().await.messages_sent += 1;

        // Wait for response with timeout
        match timeout(timeout_duration, response_rx).await {
            Ok(Ok(result)) => {
                debug!(
                    "Received roots/list response for {} in session {}",
                    request_id, self.session_id
                );
                Ok(result)
            }
            Ok(Err(_)) => {
                self.pending_roots.remove(&request_id);
                warn!(
                    "Roots/list response channel closed for {} in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::ReceiveFailed(
                    "Response channel closed".to_string(),
                ))
            }
            Err(_) => {
                self.pending_roots.remove(&request_id);
                warn!(
                    "Roots/list {} timed out in session {}",
                    request_id, self.session_id
                );
                Err(TransportError::Timeout)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::websocket_bidirectional::config::WebSocketBidirectionalConfig;

    #[tokio::test]
    async fn test_send_ping_not_connected() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let request = PingRequest {
            params: turbomcp_protocol::types::PingParams { data: None },
        };
        let result = transport.send_ping(request, None).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("WebSocket not connected")
        );
    }

    #[tokio::test]
    async fn test_send_sampling_not_connected() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let request = CreateMessageRequest {
            messages: vec![],
            model_preferences: None,
            system_prompt: None,
            include_context: None,
            temperature: None,
            max_tokens: 100,
            stop_sequences: None,
            _meta: None,
        };
        let result = transport.send_sampling(request, None).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("WebSocket not connected")
        );
    }

    #[tokio::test]
    async fn test_send_list_roots_not_connected() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let request = ListRootsRequest { _meta: None };
        let result = transport.send_list_roots(request, None).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("WebSocket not connected")
        );
    }
}
