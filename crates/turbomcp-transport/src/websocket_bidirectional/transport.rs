//! Main transport implementation for WebSocket bidirectional transport
//!
//! This module implements the Transport trait for WebSocketBidirectionalTransport,
//! providing the core send/receive operations and transport management.

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt as _};
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, trace};
use uuid::Uuid;

use super::types::WebSocketBidirectionalTransport;
use crate::core::{
    Transport, TransportCapabilities, TransportConfig, TransportError, TransportMessage,
    TransportMessageMetadata, TransportMetrics, TransportResult, TransportState, TransportType,
};
use turbomcp_core::MessageId;

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

    async fn connect(&mut self) -> TransportResult<()> {
        self.connect().await
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        self.disconnect().await
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        if let Some(ref mut writer) = *self.writer.lock().await {
            let text = String::from_utf8(message.payload.to_vec())
                .map_err(|e| TransportError::SendFailed(format!("Failed to serialize: {}", e)))?;

            writer
                .send(Message::Text(text.into()))
                .await
                .map_err(|e| TransportError::SendFailed(format!("WebSocket send failed: {}", e)))?;

            self.metrics.write().await.messages_sent += 1;
            trace!("Sent message {} in session {}", message.id, self.session_id);
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
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
                        payload: Bytes::from(data),
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
                    // Auto-reply with pong
                    if let Some(ref mut writer) = *self.writer.lock().await {
                        let _ = writer.send(Message::Pong(data)).await;
                        trace!("Replied to ping with pong in session {}", self.session_id);
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
        let metrics = self.metrics.read().await.clone();

        // TODO: Add WebSocket-specific metrics when metadata field is available:
        // - active_correlations: self.active_correlations_count()
        // - pending_elicitations: self.pending_elicitations_count()
        // - session_id: self.session_id
        // - max_message_size: self.config.max_message_size
        // - keep_alive_interval_secs: self.config.keep_alive_interval.as_secs()

        metrics
    }

    fn endpoint(&self) -> Option<String> {
        self.config.url.clone().or_else(|| {
            self.config
                .bind_addr
                .as_ref()
                .map(|addr| format!("ws://{}", addr))
        })
    }

    async fn configure(&mut self, config: TransportConfig) -> TransportResult<()> {
        // Update internal configuration based on transport config
        if let Some(keep_alive) = config.keep_alive {
            self.config.keep_alive_interval = keep_alive;
        }

        // TODO: Handle other config fields when websocket-specific config is redesigned:
        // - Use config.custom for max_message_size if needed
        // - Use config.read_timeout for elicitation_timeout if appropriate
        // - Store config metadata when metadata field is available

        trace!(
            "Updated transport configuration for session {}",
            self.session_id
        );
        Ok(())
    }
}

impl WebSocketBidirectionalTransport {
    /// Send a raw WebSocket message (for advanced use cases)
    pub async fn send_raw_message(&mut self, message: Message) -> TransportResult<()> {
        if let Some(ref mut writer) = *self.writer.lock().await {
            writer
                .send(message)
                .await
                .map_err(|e| TransportError::SendFailed(format!("WebSocket send failed: {}", e)))?;

            trace!("Sent raw WebSocket message in session {}", self.session_id);
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    /// Send a ping message manually
    pub async fn send_ping(&mut self, data: Vec<u8>) -> TransportResult<()> {
        self.send_raw_message(Message::Ping(data.into())).await
    }

    /// Send a pong message manually
    pub async fn send_pong(&mut self, data: Vec<u8>) -> TransportResult<()> {
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
        size <= self.config.max_message_size
    }

    /// Get the maximum supported message size
    pub fn max_message_size(&self) -> usize {
        self.config.max_message_size
    }

    /// Validate a message before sending
    pub fn validate_message(&self, message: &TransportMessage) -> TransportResult<()> {
        // Check message size
        if message.payload.len() > self.config.max_message_size {
            return Err(TransportError::ProtocolError(format!(
                "Message size {} exceeds maximum {}",
                message.payload.len(),
                self.config.max_message_size
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
            config: self.config.clone(),
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
        let mut transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

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
        let mut transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

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
