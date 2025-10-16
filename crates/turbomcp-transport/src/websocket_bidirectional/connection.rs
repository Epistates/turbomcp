//! Connection management for WebSocket bidirectional transport
//!
//! This module handles WebSocket connection establishment, stream setup,
//! and connection lifecycle management for both client and server modes.

use std::sync::Arc;

use futures::{SinkExt, StreamExt as _};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use super::types::{WebSocketBidirectionalTransport, WebSocketConnectionStats};
use crate::bidirectional::ConnectionState;
use crate::core::{TransportError, TransportEvent, TransportResult, TransportState, TransportType};

impl WebSocketBidirectionalTransport {
    /// Create a new WebSocket bidirectional transport
    pub async fn new(config: super::config::WebSocketBidirectionalConfig) -> TransportResult<Self> {
        // Create broadcast channel for shutdown coordination
        // Buffer size of 1 is sufficient since we only broadcast one shutdown signal
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        let (event_emitter, _) = crate::core::TransportEventEmitter::new();

        let capabilities = Self::create_capabilities(&config);

        // Capture reconnect setting before moving config
        let reconnect_enabled = config.reconnect.enabled;

        Ok(Self {
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities,
            config: Arc::new(std::sync::Mutex::new(config)),
            metrics: Arc::new(RwLock::new(crate::core::TransportMetrics::default())),
            event_emitter: Arc::new(event_emitter),
            writer: Arc::new(Mutex::new(None)),
            reader: Arc::new(Mutex::new(None)),
            correlations: Arc::new(dashmap::DashMap::new()),
            elicitations: Arc::new(dashmap::DashMap::new()),
            pending_samplings: Arc::new(dashmap::DashMap::new()),
            pending_pings: Arc::new(dashmap::DashMap::new()),
            pending_roots: Arc::new(dashmap::DashMap::new()),
            connection_state: Arc::new(RwLock::new(ConnectionState::default())),
            task_handles: Arc::new(RwLock::new(Vec::new())),
            shutdown_tx: Arc::new(shutdown_tx),
            reconnect_allowed: Arc::new(std::sync::atomic::AtomicBool::new(reconnect_enabled)),
            session_id: Uuid::new_v4().to_string(),
        })
    }

    /// Connect to a WebSocket server (client mode)
    pub async fn connect_client(&self, url: &str) -> TransportResult<()> {
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
        &self,
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
    ///
    /// Starts all essential background tasks:
    /// - Keep-alive (ping/pong)
    /// - Elicitation timeout monitor
    /// - Connection health monitor
    /// - Metrics collection
    /// - Reconnection (if enabled)
    async fn start_background_tasks(&self) {
        let mut handles = self.task_handles.write().await;

        // Keep-alive task (ping/pong)
        let keep_alive_handle = self.spawn_keep_alive_task();
        handles.push(keep_alive_handle);

        // Elicitation timeout monitor
        let timeout_handle = self.spawn_timeout_monitor();
        handles.push(timeout_handle);

        // Connection health monitor
        let health_handle = self.spawn_connection_health_monitor();
        handles.push(health_handle);

        // Metrics collection
        let metrics_handle = self.spawn_metrics_collection_task();
        handles.push(metrics_handle);

        // Reconnection task (if enabled)
        if self
            .config
            .lock()
            .expect("config mutex poisoned")
            .reconnect
            .enabled
        {
            let reconnect_handle = self.spawn_reconnection_task();
            handles.push(reconnect_handle);
        }

        info!(
            "Started {} background tasks for session {}",
            handles.len(),
            self.session_id
        );
    }

    /// Connect using the configured URL
    pub async fn connect(&self) -> TransportResult<()> {
        let url = self
            .config
            .lock()
            .expect("config mutex poisoned")
            .url
            .clone();
        if let Some(url) = url {
            self.connect_client(&url).await
        } else if self
            .config
            .lock()
            .expect("config mutex poisoned")
            .bind_addr
            .is_some()
        {
            // Server mode would be initiated by accept_connection
            Ok(())
        } else {
            Err(TransportError::ConfigurationError(
                "No URL or bind address configured".to_string(),
            ))
        }
    }

    /// Disconnect from the WebSocket
    ///
    /// This method performs a **graceful shutdown** of the WebSocket connection:
    /// 1. Sets state to Disconnecting (prevents reconnection)
    /// 2. Broadcasts shutdown signal to all background tasks
    /// 3. Sends WebSocket close frame with code 1000 (normal closure)
    /// 4. Waits for background tasks to terminate gracefully (with timeout)
    /// 5. Cleans up resources and pending operations
    pub async fn disconnect(&self) -> TransportResult<()> {
        info!(
            "🛑 Disconnecting WebSocket transport session {}",
            self.session_id
        );

        // 1. FIRST: Permanently disable reconnection (defense-in-depth)
        //    This atomic flag prevents ANY reconnection attempts, even if shutdown signals
        //    are missed or state transitions are delayed. Once set to false, reconnection
        //    is disabled for the lifetime of this transport instance.
        self.reconnect_allowed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        debug!(
            "Reconnection permanently disabled for session {}",
            self.session_id
        );

        // 2. Set state to Disconnecting
        //    This is CRITICAL - reconnection task checks this state and stops trying to reconnect
        *self.state.write().await = TransportState::Disconnecting;

        // 3. Broadcast shutdown signal to all background tasks
        //    All tasks listen via tokio::select! and will begin graceful shutdown
        let _ = self.shutdown_tx.send(()); // broadcast::send doesn't fail if no receivers
        debug!("Shutdown signal broadcast to all background tasks");

        // 4. Send WebSocket close frame with code 1000 (normal closure)
        //    This is the proper WebSocket protocol-compliant way to close
        if let Some(ref mut writer) = *self.writer.lock().await {
            use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

            let close_frame = CloseFrame {
                code: CloseCode::Normal, // 1000 - normal closure
                reason: "Client shutdown".into(),
            };

            // Send close frame to buffer
            if let Err(e) = writer
                .send(tokio_tungstenite::tungstenite::Message::Close(Some(
                    close_frame,
                )))
                .await
            {
                warn!(
                    "Failed to send close frame: {} (connection may already be closed)",
                    e
                );
                // Not fatal - connection might already be closed
            } else {
                // Flush to TCP socket to ensure immediate delivery
                if let Err(e) = writer.flush().await {
                    warn!(
                        "Failed to flush close frame: {} (connection may already be closed)",
                        e
                    );
                } else {
                    debug!("WebSocket close frame sent and flushed (code 1000)");
                }
            }
        }

        // 5. Wait for background tasks to terminate gracefully (with timeout)
        const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

        let handles = self
            .task_handles
            .write()
            .await
            .drain(..)
            .collect::<Vec<_>>();

        let shutdown_deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
        let mut graceful_count = 0;
        let mut aborted_count = 0;

        for mut handle in handles {
            let remaining =
                shutdown_deadline.saturating_duration_since(tokio::time::Instant::now());

            // Try to await the task with timeout
            match tokio::time::timeout(remaining, &mut handle).await {
                Ok(Ok(())) => {
                    graceful_count += 1;
                    trace!("Background task terminated gracefully");
                }
                Ok(Err(e)) => {
                    warn!("Background task panicked during shutdown: {:?}", e);
                    aborted_count += 1;
                }
                Err(_timeout) => {
                    // ✅ CRITICAL FIX: Actually abort non-responsive tasks
                    warn!("Background task did not respond to shutdown signal - force aborting");
                    handle.abort();
                    aborted_count += 1;
                }
            }
        }

        info!(
            "Background tasks shutdown: {} graceful, {} force aborted",
            graceful_count, aborted_count
        );

        // 6. Clear state and pending operations
        self.correlations.clear();
        self.elicitations.clear();

        *self.writer.lock().await = None;
        *self.reader.lock().await = None;
        *self.state.write().await = TransportState::Disconnected;

        // Update connection state metadata
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
            endpoint: self.session_id.clone(),
            reason: Some("User-initiated disconnect".to_string()),
        });

        info!(
            "✅ WebSocket transport disconnected successfully (session {})",
            self.session_id
        );
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
        if !self
            .config
            .lock()
            .expect("config mutex poisoned")
            .reconnect
            .enabled
        {
            return Err(TransportError::NotAvailable(
                "Reconnection is disabled".to_string(),
            ));
        }

        let url = self
            .config
            .lock()
            .expect("config mutex poisoned")
            .url
            .clone()
            .ok_or_else(|| {
                TransportError::ConfigurationError("No URL configured for reconnection".to_string())
            })?;

        let mut retry_count = 0;
        let mut delay = self
            .config
            .lock()
            .expect("config mutex poisoned")
            .reconnect
            .initial_delay;

        while retry_count
            < self
                .config
                .lock()
                .expect("config mutex poisoned")
                .reconnect
                .max_retries
        {
            info!(
                "Attempting reconnection {} of {}",
                retry_count + 1,
                self.config
                    .lock()
                    .expect("config mutex poisoned")
                    .reconnect
                    .max_retries
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

                    if retry_count
                        < self
                            .config
                            .lock()
                            .expect("config mutex poisoned")
                            .reconnect
                            .max_retries
                    {
                        tokio::time::sleep(delay).await;

                        // Exponential backoff
                        delay = std::time::Duration::from_secs_f64(
                            (delay.as_secs_f64()
                                * self
                                    .config
                                    .lock()
                                    .expect("config mutex poisoned")
                                    .reconnect
                                    .backoff_factor)
                                .min(
                                    self.config
                                        .lock()
                                        .expect("config mutex poisoned")
                                        .reconnect
                                        .max_delay
                                        .as_secs_f64(),
                                ),
                        );
                    }
                }
            }
        }

        Err(TransportError::ConnectionFailed(format!(
            "Reconnection failed after {} attempts",
            self.config
                .lock()
                .expect("config mutex poisoned")
                .reconnect
                .max_retries
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
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

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
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Should be able to disconnect even if not connected
        let result = transport.disconnect().await;
        assert!(result.is_ok());
    }
}
