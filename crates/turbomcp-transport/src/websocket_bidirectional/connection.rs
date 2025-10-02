//! Connection management for WebSocket bidirectional transport
//!
//! This module handles WebSocket connection establishment, stream setup,
//! and connection lifecycle management for both client and server modes.

use std::sync::Arc;

use futures::{SinkExt, StreamExt as _};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{info, warn};
use uuid::Uuid;

use super::types::{WebSocketBidirectionalTransport, WebSocketConnectionStats};
use crate::bidirectional::ConnectionState;
use crate::core::{TransportError, TransportEvent, TransportResult, TransportState, TransportType};

impl WebSocketBidirectionalTransport {
    /// Create a new WebSocket bidirectional transport
    pub async fn new(config: super::config::WebSocketBidirectionalConfig) -> TransportResult<Self> {
        let (_shutdown_tx, _shutdown_rx) = mpsc::channel(1);
        let (event_emitter, _) = crate::core::TransportEventEmitter::new();

        let capabilities = Self::create_capabilities(&config);

        Ok(Self {
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities,
            config,
            metrics: Arc::new(RwLock::new(crate::core::TransportMetrics::default())),
            event_emitter: Arc::new(event_emitter),
            writer: Arc::new(Mutex::new(None)),
            reader: Arc::new(Mutex::new(None)),
            correlations: Arc::new(dashmap::DashMap::new()),
            elicitations: Arc::new(dashmap::DashMap::new()),
            connection_state: Arc::new(RwLock::new(ConnectionState::default())),
            task_handles: Arc::new(RwLock::new(Vec::new())),
            shutdown_tx: Some(_shutdown_tx),
            session_id: Uuid::new_v4().to_string(),
        })
    }

    /// Connect to a WebSocket server (client mode)
    pub async fn connect_client(&mut self, url: &str) -> TransportResult<()> {
        info!("Connecting to WebSocket server at {}", url);

        let (stream, _response) = connect_async(url).await.map_err(|e| {
            TransportError::ConnectionFailed(format!("WebSocket connection failed: {}", e))
        })?;

        self.setup_stream(stream).await?;

        info!("WebSocket client connected successfully");
        Ok(())
    }

    /// Accept a WebSocket connection (server mode)
    pub async fn accept_connection(&mut self, _stream: TcpStream) -> TransportResult<()> {
        // Current implementation: Client mode only
        // Server mode requires handling different stream types:
        // accept_async -> WebSocketStream<TcpStream> vs connect_async -> WebSocketStream<MaybeTlsStream<TcpStream>>
        // Architecture supports this via trait abstraction over stream types
        Err(TransportError::NotAvailable(
            "Server mode not yet implemented".to_string(),
        ))
    }

    /// Setup the WebSocket stream and start background tasks
    pub async fn setup_stream(
        &mut self,
        stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> TransportResult<()> {
        let (writer, reader) = stream.split();

        *self.writer.lock().await = Some(writer);
        *self.reader.lock().await = Some(reader);
        *self.state.write().await = TransportState::Connected;

        // Update connection state
        {
            let mut conn_state = self.connection_state.write().await;
            conn_state.server_initiated_enabled = true;
            conn_state
                .metadata
                .insert("session_id".to_string(), json!(self.session_id));
            conn_state
                .metadata
                .insert("connected_at".to_string(), json!(chrono::Utc::now()));
            conn_state.metadata.insert(
                "transport_type".to_string(),
                json!("websocket_bidirectional"),
            );
        }

        // Start background tasks
        self.start_background_tasks().await;

        // Emit connected event
        self.event_emitter.emit(TransportEvent::Connected {
            transport_type: TransportType::WebSocket,
            endpoint: "websocket".to_string(),
        });

        info!(
            "WebSocket stream setup completed for session {}",
            self.session_id
        );
        Ok(())
    }

    /// Start background tasks for message processing
    async fn start_background_tasks(&self) {
        let mut handles = self.task_handles.write().await;

        // Keep-alive task
        let keep_alive_handle = self.spawn_keep_alive_task();
        handles.push(keep_alive_handle);

        // Elicitation timeout monitor
        let timeout_handle = self.spawn_timeout_monitor();
        handles.push(timeout_handle);

        // Reconnection task (if enabled)
        if self.config.reconnect.enabled {
            let reconnect_handle = self.spawn_reconnection_task();
            handles.push(reconnect_handle);
        }

        info!("Started {} background tasks", handles.len());
    }

    /// Connect using the configured URL
    pub async fn connect(&mut self) -> TransportResult<()> {
        let url = self.config.url.clone();
        if let Some(url) = url {
            self.connect_client(&url).await
        } else if self.config.bind_addr.is_some() {
            // Server mode would be initiated by accept_connection
            Ok(())
        } else {
            Err(TransportError::ConfigurationError(
                "No URL or bind address configured".to_string(),
            ))
        }
    }

    /// Disconnect from the WebSocket
    pub async fn disconnect(&mut self) -> TransportResult<()> {
        info!("Disconnecting WebSocket transport");

        *self.state.write().await = TransportState::Disconnecting;

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Close WebSocket
        if let Some(ref mut writer) = *self.writer.lock().await {
            let _ = writer
                .send(tokio_tungstenite::tungstenite::Message::Close(None))
                .await;
        }

        // Cancel background tasks
        let handles = self
            .task_handles
            .write()
            .await
            .drain(..)
            .collect::<Vec<_>>();
        for handle in handles {
            handle.abort();
        }

        // Clear state
        self.correlations.clear();
        self.elicitations.clear();

        *self.writer.lock().await = None;
        *self.reader.lock().await = None;
        *self.state.write().await = TransportState::Disconnected;

        // Update connection state
        {
            let mut conn_state = self.connection_state.write().await;
            conn_state.server_initiated_enabled = false;
            conn_state
                .metadata
                .insert("disconnected_at".to_string(), json!(chrono::Utc::now()));
        }

        // Emit disconnected event
        self.event_emitter.emit(TransportEvent::Disconnected {
            transport_type: TransportType::WebSocket,
            endpoint: "websocket".to_string(),
            reason: Some("Disconnected by user".to_string()),
        });

        info!("WebSocket transport disconnected successfully");
        Ok(())
    }

    /// Get connection statistics
    pub async fn get_connection_stats(&self) -> WebSocketConnectionStats {
        let metrics = self.metrics.read().await;
        let state = self.state.read().await;
        let conn_state = self.connection_state.read().await;

        let mut stats = WebSocketConnectionStats {
            messages_sent: metrics.messages_sent,
            messages_received: metrics.messages_received,
            connection_state: state.clone(),
            ..Default::default()
        };

        // Extract connection time from metadata
        if let Some(connected_at) = conn_state.metadata.get("connected_at")
            && let Ok(timestamp) =
                serde_json::from_value::<chrono::DateTime<chrono::Utc>>(connected_at.clone())
        {
            stats.connected_at = Some(timestamp.into());
        }

        stats
    }

    /// Check if the transport is ready for operations
    pub async fn is_ready(&self) -> bool {
        matches!(*self.state.read().await, TransportState::Connected)
            && self.is_writer_connected().await
            && self.is_reader_connected().await
    }

    /// Reconnect with exponential backoff
    pub async fn reconnect(&mut self) -> TransportResult<()> {
        if !self.config.reconnect.enabled {
            return Err(TransportError::NotAvailable(
                "Reconnection is disabled".to_string(),
            ));
        }

        let url = self.config.url.clone().ok_or_else(|| {
            TransportError::ConfigurationError("No URL configured for reconnection".to_string())
        })?;

        let mut retry_count = 0;
        let mut delay = self.config.reconnect.initial_delay;

        while retry_count < self.config.reconnect.max_retries {
            info!(
                "Attempting reconnection {} of {}",
                retry_count + 1,
                self.config.reconnect.max_retries
            );

            // Update metrics
            {
                let mut stats = WebSocketConnectionStats::new();
                stats.record_reconnection_attempt();
            }

            match self.connect_client(&url).await {
                Ok(()) => {
                    info!("Reconnection successful after {} attempts", retry_count + 1);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Reconnection attempt {} failed: {}", retry_count + 1, e);
                    retry_count += 1;

                    if retry_count < self.config.reconnect.max_retries {
                        tokio::time::sleep(delay).await;

                        // Exponential backoff
                        delay = std::time::Duration::from_secs_f64(
                            (delay.as_secs_f64() * self.config.reconnect.backoff_factor)
                                .min(self.config.reconnect.max_delay.as_secs_f64()),
                        );
                    }
                }
            }
        }

        Err(TransportError::ConnectionFailed(format!(
            "Reconnection failed after {} attempts",
            self.config.reconnect.max_retries
        )))
    }

    /// Force close the connection immediately
    pub async fn force_close(&mut self) {
        warn!("Force closing WebSocket connection");

        *self.state.write().await = TransportState::Disconnected;

        // Abort all tasks immediately
        let handles = self
            .task_handles
            .write()
            .await
            .drain(..)
            .collect::<Vec<_>>();
        for handle in handles {
            handle.abort();
        }

        // Clear all state
        self.correlations.clear();
        self.elicitations.clear();
        *self.writer.lock().await = None;
        *self.reader.lock().await = None;

        // Emit disconnected event
        self.event_emitter.emit(TransportEvent::Disconnected {
            transport_type: TransportType::WebSocket,
            endpoint: "websocket".to_string(),
            reason: Some("Force closed".to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Transport;
    use crate::websocket_bidirectional::config::WebSocketBidirectionalConfig;

    #[tokio::test]
    async fn test_websocket_transport_creation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert_eq!(transport.transport_type(), TransportType::WebSocket);
        assert!(transport.capabilities().supports_bidirectional);
        assert!(!transport.session_id().is_empty());
    }

    #[tokio::test]
    async fn test_connection_config_validation() {
        // Test with no URL or bind address
        let config = WebSocketBidirectionalConfig::default();
        let mut transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let result = transport.connect().await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No URL or bind address")
        );
    }

    #[tokio::test]
    async fn test_connection_stats() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        let stats = transport.get_connection_stats().await;
        assert_eq!(stats.messages_sent, 0);
        assert_eq!(stats.messages_received, 0);
        assert!(matches!(
            stats.connection_state,
            TransportState::Disconnected
        ));
    }

    #[tokio::test]
    async fn test_transport_readiness() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Transport should not be ready initially
        assert!(!transport.is_ready().await);
    }

    #[tokio::test]
    async fn test_disconnect_without_connection() {
        let config = WebSocketBidirectionalConfig::default();
        let mut transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should be able to disconnect even if not connected
        let result = transport.disconnect().await;
        assert!(result.is_ok());
    }
}
