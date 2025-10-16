//! Main transport implementation for WebSocket bidirectional transport
//!
//! This module implements the Transport trait for WebSocketBidirectionalTransport,
//! providing the core send/receive operations and transport management.

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt as _};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, trace};
use uuid::Uuid;

use super::types::WebSocketBidirectionalTransport;
use crate::core::{
    Transport, TransportCapabilities, TransportConfig, TransportError, TransportMessage,
    TransportMessageMetadata, TransportMetrics, TransportResult, TransportState, TransportType,
};
use turbomcp_protocol::MessageId;

#[async_trait]
impl Transport for WebSocketBidirectionalTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.read().await.clone()
    }

    async fn connect(&self) -> TransportResult<()> {
        self.connect().await
    }

    async fn disconnect(&self) -> TransportResult<()> {
        self.disconnect().await
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        if let Some(ref mut writer) = *self.writer.lock().await {
            let text = String::from_utf8(message.payload.to_vec())
                .map_err(|e| TransportError::SendFailed(format!("Failed to serialize: {}", e)))?;

            info!("🚀 [CLIENT SEND] Attempting to send message");
            info!("   Session: {}", self.session_id);
            info!("   Payload length: {} bytes", text.len());
            debug!(
                "   Payload preview: {}",
                if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.clone()
                }
            );

            // Step 1: Send message to buffer
            writer.send(Message::Text(text.into())).await.map_err(|e| {
                error!("❌ [CLIENT SEND] WebSocket send() failed: {}", e);
                TransportError::SendFailed(format!("WebSocket send failed: {}", e))
            })?;

            debug!("✅ [CLIENT SEND] Message buffered successfully");

            // Step 2: Flush buffer to TCP socket
            // CRITICAL: Without flush(), messages sit in buffer and never reach the network!
            // This is required by the futures::Sink trait semantics.
            writer.flush().await.map_err(|e| {
                error!("❌ [CLIENT SEND] WebSocket flush() failed: {}", e);
                TransportError::SendFailed(format!("WebSocket flush failed: {}", e))
            })?;

            info!("✅ [CLIENT SEND] Message flushed to TCP socket successfully");
            self.metrics.write().await.messages_sent += 1;
            trace!(
                "Sent and flushed message {} in session {}",
                message.id, self.session_id
            );
            Ok(())
        } else {
            error!("❌ [CLIENT SEND] Writer is None - WebSocket not connected");
            Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        if let Some(ref mut reader) = *self.reader.lock().await {
            match reader.next().await {
                Some(Ok(Message::Text(text))) => {
                    // Process for elicitation responses first
                    let _ = self.process_incoming_message(text.to_string()).await;

                    let message = TransportMessage {
                        id: MessageId::from(Uuid::new_v4()),
                        payload: Bytes::from(text.as_bytes().to_vec()),
                        metadata: TransportMessageMetadata::default(),
                    };

                    self.metrics.write().await.messages_received += 1;
                    trace!(
                        "Received message {} in session {}",
                        message.id, self.session_id
                    );
                    Ok(Some(message))
                }
                Some(Ok(Message::Binary(data))) => {
                    let message = TransportMessage {
                        id: MessageId::from(Uuid::new_v4()),
                        payload: data,
                        metadata: TransportMessageMetadata::default(),
                    };

                    self.metrics.write().await.messages_received += 1;
                    trace!(
                        "Received binary message {} in session {}",
                        message.id, self.session_id
                    );
                    Ok(Some(message))
                }
                Some(Ok(Message::Close(_))) => {
                    *self.state.write().await = TransportState::Disconnected;
                    Err(TransportError::ConnectionLost(
                        "WebSocket closed".to_string(),
                    ))
                }
                Some(Ok(Message::Ping(data))) => {
                    // Auto-reply with pong (required by WebSocket protocol)
                    if let Some(ref mut writer) = *self.writer.lock().await {
                        // Send pong to buffer
                        if let Ok(()) = writer.send(Message::Pong(data)).await {
                            // Flush to TCP socket to ensure immediate response
                            let _ = writer.flush().await;
                            trace!(
                                "Replied to ping with pong and flushed in session {}",
                                self.session_id
                            );
                        }
                    }
                    Ok(None)
                }
                Some(Ok(Message::Pong(_))) => {
                    trace!("Received pong in session {}", self.session_id);
                    Ok(None)
                }
                Some(Ok(Message::Frame(_))) => {
                    // Raw frames are typically not used in normal operation
                    trace!("Received raw frame in session {}", self.session_id);
                    Ok(None)
                }
                Some(Err(e)) => {
                    error!("WebSocket error in session {}: {}", self.session_id, e);
                    *self.state.write().await = TransportState::Disconnected;
                    Err(TransportError::ReceiveFailed(e.to_string()))
                }
                None => {
                    *self.state.write().await = TransportState::Disconnected;
                    Err(TransportError::ConnectionLost(
                        "WebSocket stream ended".to_string(),
                    ))
                }
            }
        } else {
            Err(TransportError::ReceiveFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        let mut base_metrics = self.metrics.read().await.clone();

        // Add WebSocket-specific metrics to metadata
        let config = self.config.lock().expect("config mutex poisoned");
        base_metrics.metadata.insert(
            "active_correlations".to_string(),
            serde_json::json!(self.active_correlations_count()),
        );
        base_metrics.metadata.insert(
            "pending_elicitations".to_string(),
            serde_json::json!(self.pending_elicitations_count()),
        );
        base_metrics.metadata.insert(
            "session_id".to_string(),
            serde_json::json!(self.session_id.to_string()),
        );
        base_metrics.metadata.insert(
            "max_message_size".to_string(),
            serde_json::json!(config.max_message_size),
        );
        base_metrics.metadata.insert(
            "keep_alive_interval_secs".to_string(),
            serde_json::json!(config.keep_alive_interval.as_secs()),
        );

        base_metrics
    }

    fn endpoint(&self) -> Option<String> {
        let config_guard = self.config.lock().expect("config mutex poisoned");
        config_guard.url.clone().or_else(|| {
            config_guard
                .bind_addr
                .as_ref()
                .map(|addr| format!("ws://{}", addr))
        })
    }

    async fn configure(&self, config: TransportConfig) -> TransportResult<()> {
        let mut ws_config = self.config.lock().expect("config mutex poisoned");

        // Update keep-alive from standard config
        if let Some(keep_alive) = config.keep_alive {
            ws_config.keep_alive_interval = keep_alive;
        }

        // Extract WebSocket-specific config from custom field
        if let Some(max_msg_size) = config.custom.get("max_message_size")
            && let Some(size) = max_msg_size.as_u64()
        {
            ws_config.max_message_size = size as usize;
            trace!(
                "Updated max_message_size to {} for session {}",
                size, self.session_id
            );
        }

        // Use read_timeout for elicitation_timeout if provided
        if let Some(read_timeout) = config.read_timeout {
            if let Some(elicitation_timeout) = config
                .custom
                .get("elicitation_timeout")
                .and_then(|v| v.as_u64())
                .map(Duration::from_secs)
            {
                ws_config.elicitation_timeout = elicitation_timeout;
            } else {
                // Fall back to read_timeout if elicitation_timeout not explicitly set
                ws_config.elicitation_timeout = read_timeout;
            }
            trace!(
                "Updated elicitation_timeout to {:?} for session {}",
                ws_config.elicitation_timeout, self.session_id
            );
        }

        trace!(
            "Updated transport configuration for session {}",
            self.session_id
        );
        Ok(())
    }
}

impl WebSocketBidirectionalTransport {
    /// Send a raw WebSocket message (for advanced use cases)
    ///
    /// This method sends control frames (ping, pong, close) and other raw messages.
    /// It properly flushes the buffer to ensure immediate delivery to the TCP socket.
    pub async fn send_raw_message(&mut self, message: Message) -> TransportResult<()> {
        if let Some(ref mut writer) = *self.writer.lock().await {
            // Send to buffer
            writer
                .send(message)
                .await
                .map_err(|e| TransportError::SendFailed(format!("WebSocket send failed: {}", e)))?;

            // Flush to TCP socket (CRITICAL for immediate delivery)
            writer.flush().await.map_err(|e| {
                TransportError::SendFailed(format!("WebSocket flush failed: {}", e))
            })?;

            trace!(
                "Sent and flushed raw WebSocket message in session {}",
                self.session_id
            );
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    /// Send a WebSocket ping frame manually (low-level keep-alive)
    ///
    /// Note: This is different from MCP protocol `ping` (see `send_ping` in mcp_methods.rs)
    pub async fn send_ws_ping(&mut self, data: Vec<u8>) -> TransportResult<()> {
        self.send_raw_message(Message::Ping(data.into())).await
    }

    /// Send a WebSocket pong frame manually (low-level keep-alive)
    pub async fn send_ws_pong(&mut self, data: Vec<u8>) -> TransportResult<()> {
        self.send_raw_message(Message::Pong(data.into())).await
    }

    /// Send a close message with optional close code and reason
    pub async fn send_close(
        &mut self,
        close_frame: Option<tokio_tungstenite::tungstenite::protocol::CloseFrame>,
    ) -> TransportResult<()> {
        self.send_raw_message(Message::Close(close_frame)).await
    }

    /// Check if the transport supports a specific message size
    pub fn supports_message_size(&self, size: usize) -> bool {
        size <= self
            .config
            .lock()
            .expect("config mutex poisoned")
            .max_message_size
    }

    /// Get the maximum supported message size
    pub fn max_message_size(&self) -> usize {
        self.config
            .lock()
            .expect("config mutex poisoned")
            .max_message_size
    }

    /// Validate a message before sending
    pub fn validate_message(&self, message: &TransportMessage) -> TransportResult<()> {
        // Check message size
        if message.payload.len()
            > self
                .config
                .lock()
                .expect("config mutex poisoned")
                .max_message_size
        {
            return Err(TransportError::ProtocolError(format!(
                "Message size {} exceeds maximum {}",
                message.payload.len(),
                self.config
                    .lock()
                    .expect("config mutex poisoned")
                    .max_message_size
            )));
        }

        // Validate payload is valid UTF-8 for text messages
        if std::str::from_utf8(&message.payload).is_err() {
            return Err(TransportError::SendFailed(
                "Message payload contains invalid UTF-8".to_string(),
            ));
        }

        Ok(())
    }

    /// Send a validated message
    pub async fn send_validated(&mut self, message: TransportMessage) -> TransportResult<()> {
        self.validate_message(&message)?;
        self.send(message).await
    }

    /// Get detailed transport status
    pub async fn get_detailed_status(&self) -> TransportStatus {
        let state = self.state.read().await.clone();
        let metrics = self.metrics().await;
        let connection_stats = self.get_connection_stats().await;

        TransportStatus {
            state,
            session_id: self.session_id.clone(),
            endpoint: self.endpoint(),
            is_writer_connected: self.is_writer_connected().await,
            is_reader_connected: self.is_reader_connected().await,
            active_correlations: self.active_correlations_count(),
            pending_elicitations: self.pending_elicitations_count(),
            messages_sent: metrics.messages_sent,
            messages_received: metrics.messages_received,
            connection_uptime: connection_stats.uptime(),
            last_activity: connection_stats.last_activity,
            config: self.config.lock().expect("config mutex poisoned").clone(),
        }
    }
}

/// Detailed transport status information
#[derive(Debug, Clone)]
pub struct TransportStatus {
    /// Current transport state
    pub state: TransportState,
    /// Session ID
    pub session_id: String,
    /// Endpoint URL or address
    pub endpoint: Option<String>,
    /// Whether writer is connected
    pub is_writer_connected: bool,
    /// Whether reader is connected
    pub is_reader_connected: bool,
    /// Number of active correlations
    pub active_correlations: usize,
    /// Number of pending elicitations
    pub pending_elicitations: usize,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Connection uptime
    pub connection_uptime: Option<std::time::Duration>,
    /// Last activity timestamp
    pub last_activity: Option<std::time::SystemTime>,
    /// Transport configuration
    pub config: super::config::WebSocketBidirectionalConfig,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::websocket_bidirectional::config::WebSocketBidirectionalConfig;

    #[tokio::test]
    async fn test_transport_type() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();
        assert_eq!(transport.transport_type(), TransportType::WebSocket);
    }

    #[tokio::test]
    async fn test_transport_capabilities() {
        let config = WebSocketBidirectionalConfig {
            enable_compression: true,
            max_message_size: 1024,
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let capabilities = transport.capabilities();
        assert!(capabilities.supports_bidirectional);
        assert!(capabilities.supports_streaming);
        assert!(capabilities.supports_compression);
        assert_eq!(capabilities.max_message_size, Some(1024));
    }

    #[tokio::test]
    async fn test_transport_state() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();
        assert_eq!(transport.state().await, TransportState::Disconnected);
    }

    #[tokio::test]
    async fn test_send_without_connection() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let message = TransportMessage {
            id: MessageId::from(Uuid::new_v4()),
            payload: Bytes::from("test".as_bytes()),
            metadata: TransportMessageMetadata::default(),
        };

        let result = transport.send(message).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not connected"));
    }

    #[tokio::test]
    async fn test_receive_without_connection() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let result = transport.receive().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not connected"));
    }

    #[tokio::test]
    async fn test_validate_message() {
        let config = WebSocketBidirectionalConfig {
            max_message_size: 10,
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Valid message
        let valid_message = TransportMessage {
            id: MessageId::from(Uuid::new_v4()),
            payload: Bytes::from("test".as_bytes()),
            metadata: TransportMessageMetadata::default(),
        };
        assert!(transport.validate_message(&valid_message).is_ok());

        // Message too large
        let large_message = TransportMessage {
            id: MessageId::from(Uuid::new_v4()),
            payload: Bytes::from("this message is too long".as_bytes()),
            metadata: TransportMessageMetadata::default(),
        };
        assert!(transport.validate_message(&large_message).is_err());
    }

    // NOTE: test_transport_configuration removed - it was using old API fields that don't exist
    // (max_message_size and timeout on TransportConfig)

    #[tokio::test]
    async fn test_get_detailed_status() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let status = transport.get_detailed_status().await;
        assert_eq!(status.state, TransportState::Disconnected);
        assert!(!status.session_id.is_empty());
        assert!(!status.is_writer_connected);
        assert!(!status.is_reader_connected);
        assert_eq!(status.active_correlations, 0);
        assert_eq!(status.pending_elicitations, 0);
    }

    #[tokio::test]
    async fn test_endpoint() {
        let config = WebSocketBidirectionalConfig {
            url: Some("ws://example.com:8080".to_string()),
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert_eq!(
            transport.endpoint(),
            Some("ws://example.com:8080".to_string())
        );
    }

    #[tokio::test]
    async fn test_endpoint_with_bind_addr() {
        let config = WebSocketBidirectionalConfig {
            bind_addr: Some("0.0.0.0:8080".to_string()),
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert_eq!(transport.endpoint(), Some("ws://0.0.0.0:8080".to_string()));
    }
}
