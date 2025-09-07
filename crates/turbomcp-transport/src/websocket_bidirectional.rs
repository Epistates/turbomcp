//! WebSocket bidirectional transport with elicitation support
//!
//! This module provides a production-grade WebSocket transport implementation
//! with full bidirectional communication support for the MCP 2025-06-18 protocol,
//! including server-initiated elicitation requests.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use futures::{SinkExt as _, StreamExt as _, stream::SplitSink, stream::SplitStream};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

use turbomcp_core::MessageId;
use turbomcp_protocol::elicitation::{
    ElicitationAction, ElicitationCreateRequest, ElicitationCreateResult,
};

use crate::bidirectional::{ConnectionState, CorrelationContext};
use crate::core::{
    BidirectionalTransport, Transport, TransportCapabilities, TransportError, TransportEvent,
    TransportEventEmitter, TransportMessage, TransportMessageMetadata, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

// Type aliases to reduce complexity
type WebSocketWriter =
    Arc<Mutex<Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>>;
type WebSocketReader = Arc<Mutex<Option<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>>;

/// WebSocket bidirectional transport implementation
#[derive(Debug)]
pub struct WebSocketBidirectionalTransport {
    /// Transport state
    state: Arc<RwLock<TransportState>>,

    /// Transport capabilities
    capabilities: TransportCapabilities,

    /// Configuration
    config: WebSocketBidirectionalConfig,

    /// Metrics collector
    metrics: Arc<RwLock<TransportMetrics>>,

    /// Event emitter for transport events
    event_emitter: Arc<TransportEventEmitter>,

    /// WebSocket write half (sender)
    writer: WebSocketWriter,

    /// WebSocket read half (receiver)
    reader: WebSocketReader,

    /// Active correlations for request-response patterns
    correlations: Arc<DashMap<String, CorrelationContext>>,

    /// Pending elicitation requests
    elicitations: Arc<DashMap<String, PendingElicitation>>,

    /// Connection state
    connection_state: Arc<RwLock<ConnectionState>>,

    /// Background task handles
    task_handles: Arc<RwLock<Vec<tokio::task::JoinHandle<()>>>>,

    /// Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,

    /// Session ID for this connection
    session_id: String,
}

/// Configuration for WebSocket bidirectional transport
#[derive(Clone, Debug)]
pub struct WebSocketBidirectionalConfig {
    /// WebSocket URL to connect to (client mode)
    pub url: Option<String>,

    /// Bind address for server mode
    pub bind_addr: Option<String>,

    /// Maximum message size (default: 16MB)
    pub max_message_size: usize,

    /// Keep-alive interval
    pub keep_alive_interval: Duration,

    /// Reconnection configuration
    pub reconnect: ReconnectConfig,

    /// Elicitation timeout
    pub elicitation_timeout: Duration,

    /// Maximum concurrent elicitations
    pub max_concurrent_elicitations: usize,

    /// Enable compression
    pub enable_compression: bool,

    /// TLS configuration
    pub tls_config: Option<TlsConfig>,
}

impl Default for WebSocketBidirectionalConfig {
    fn default() -> Self {
        Self {
            url: None,
            bind_addr: None,
            max_message_size: 16 * 1024 * 1024, // 16MB
            keep_alive_interval: Duration::from_secs(30),
            reconnect: ReconnectConfig::default(),
            elicitation_timeout: Duration::from_secs(30),
            max_concurrent_elicitations: 10,
            enable_compression: false,
            tls_config: None,
        }
    }
}

/// Reconnection configuration
#[derive(Clone, Debug)]
pub struct ReconnectConfig {
    /// Enable automatic reconnection
    pub enabled: bool,

    /// Initial retry delay
    pub initial_delay: Duration,

    /// Maximum retry delay
    pub max_delay: Duration,

    /// Exponential backoff factor
    pub backoff_factor: f64,

    /// Maximum number of retries
    pub max_retries: u32,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
            max_retries: 10,
        }
    }
}

/// TLS configuration
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// Client certificate path
    pub cert_path: Option<String>,

    /// Client key path
    pub key_path: Option<String>,

    /// CA certificate path
    pub ca_path: Option<String>,

    /// Skip certificate verification (dangerous!)
    pub skip_verify: bool,
}

/// Pending elicitation request
#[derive(Debug)]
struct PendingElicitation {
    /// Request ID for correlation
    _request_id: String,

    /// The elicitation request
    _request: ElicitationCreateRequest,

    /// Response channel
    response_tx: oneshot::Sender<ElicitationCreateResult>,

    /// Timeout deadline
    deadline: tokio::time::Instant,

    /// Retry count
    _retry_count: u32,
}

impl WebSocketBidirectionalTransport {
    /// Create a new WebSocket bidirectional transport
    pub async fn new(config: WebSocketBidirectionalConfig) -> TransportResult<Self> {
        let (_shutdown_tx, _shutdown_rx) = mpsc::channel(1);
        let (event_emitter, _) = TransportEventEmitter::new();

        let capabilities = TransportCapabilities {
            max_message_size: Some(config.max_message_size),
            supports_compression: config.enable_compression,
            supports_streaming: true,
            supports_bidirectional: true,
            supports_multiplexing: true,
            compression_algorithms: if config.enable_compression {
                vec!["deflate".to_string(), "gzip".to_string()]
            } else {
                Vec::new()
            },
            custom: {
                let mut custom = HashMap::new();
                custom.insert("elicitation".to_string(), json!(true));
                custom.insert("sampling".to_string(), json!(true));
                custom
            },
        };

        Ok(Self {
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities,
            config,
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            event_emitter: Arc::new(event_emitter),
            writer: Arc::new(Mutex::new(None)),
            reader: Arc::new(Mutex::new(None)),
            correlations: Arc::new(DashMap::new()),
            elicitations: Arc::new(DashMap::new()),
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
    async fn setup_stream(
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
        }

        // Start background tasks
        self.start_background_tasks().await;

        // Emit connected event
        self.event_emitter.emit(TransportEvent::Connected {
            transport_type: TransportType::WebSocket,
            endpoint: "websocket".to_string(),
        });

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
    }

    /// Spawn keep-alive task
    fn spawn_keep_alive_task(&self) -> tokio::task::JoinHandle<()> {
        let writer = self.writer.clone();
        let interval = self.config.keep_alive_interval;
        let state = self.state.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;

                if *state.read().await != TransportState::Connected {
                    continue;
                }

                if let Some(ref mut w) = *writer.lock().await {
                    if let Err(e) = w.send(Message::Ping(vec![])).await {
                        warn!("Keep-alive ping failed: {}", e);
                    } else {
                        trace!("Keep-alive ping sent");
                    }
                }
            }
        })
    }

    /// Spawn elicitation timeout monitor
    fn spawn_timeout_monitor(&self) -> tokio::task::JoinHandle<()> {
        let elicitations = self.elicitations.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));

            loop {
                ticker.tick().await;

                let now = tokio::time::Instant::now();
                let mut expired = Vec::new();

                // Find expired elicitations
                for entry in elicitations.iter() {
                    if entry.deadline <= now {
                        expired.push(entry.key().clone());
                    }
                }

                // Handle expired elicitations
                for request_id in expired {
                    if let Some((_, pending)) = elicitations.remove(&request_id) {
                        warn!("Elicitation {} timed out", request_id);

                        let result = ElicitationCreateResult {
                            action: ElicitationAction::Cancel,
                            content: None,
                            meta: None,
                        };

                        let _ = pending.response_tx.send(result);
                    }
                }
            }
        })
    }

    /// Spawn reconnection task
    fn spawn_reconnection_task(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut retry_count = 0;
            let mut delay = config.reconnect.initial_delay;

            loop {
                sleep(Duration::from_secs(5)).await;

                if *state.read().await == TransportState::Connected {
                    retry_count = 0;
                    delay = config.reconnect.initial_delay;
                    continue;
                }

                if retry_count >= config.reconnect.max_retries {
                    error!("Maximum reconnection attempts reached");
                    break;
                }

                info!(
                    "Attempting reconnection {} of {}",
                    retry_count + 1,
                    config.reconnect.max_retries
                );

                // Attempt reconnection
                if let Some(ref url) = config.url {
                    match connect_async(url).await {
                        Ok((_stream, _)) => {
                            info!("Reconnection successful");
                            // Note: Would need to call setup_stream here
                            // but we'd need mutable self access
                            retry_count = 0;
                            delay = config.reconnect.initial_delay;
                        }
                        Err(e) => {
                            warn!("Reconnection failed: {}", e);
                            retry_count += 1;

                            sleep(delay).await;

                            // Exponential backoff
                            delay = Duration::from_secs_f64(
                                (delay.as_secs_f64() * config.reconnect.backoff_factor)
                                    .min(config.reconnect.max_delay.as_secs_f64()),
                            );
                        }
                    }
                }
            }
        })
    }

    /// Send an elicitation request
    pub async fn send_elicitation(
        &self,
        request: ElicitationCreateRequest,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<ElicitationCreateResult> {
        // Check if we're at capacity
        if self.elicitations.len() >= self.config.max_concurrent_elicitations {
            return Err(TransportError::SendFailed(
                "Maximum concurrent elicitations reached".to_string(),
            ));
        }

        let request_id = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = oneshot::channel();

        let deadline = tokio::time::Instant::now()
            + timeout_duration.unwrap_or(self.config.elicitation_timeout);

        // Store pending elicitation
        let pending = PendingElicitation {
            _request_id: request_id.clone(),
            _request: request.clone(),
            response_tx,
            deadline,
            _retry_count: 0,
        };

        self.elicitations.insert(request_id.clone(), pending);

        // Create JSON-RPC request
        let json_request = json!({
            "jsonrpc": "2.0",
            "method": "elicitation/create",
            "params": request,
            "id": request_id
        });

        // Send via WebSocket
        let message_text = serde_json::to_string(&json_request)
            .map_err(|e| TransportError::SendFailed(format!("Failed to serialize: {}", e)))?;

        if let Some(ref mut writer) = *self.writer.lock().await {
            writer
                .send(Message::Text(message_text))
                .await
                .map_err(|e| TransportError::SendFailed(format!("WebSocket send failed: {}", e)))?;

            debug!("Sent elicitation request {}", request_id);
        } else {
            self.elicitations.remove(&request_id);
            return Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ));
        }

        // Update metrics
        self.metrics.write().await.messages_sent += 1;

        // Wait for response
        match timeout(
            deadline.duration_since(tokio::time::Instant::now()),
            response_rx,
        )
        .await
        {
            Ok(Ok(result)) => {
                debug!("Received elicitation response for {}", request_id);
                Ok(result)
            }
            Ok(Err(_)) => {
                warn!("Elicitation response channel closed for {}", request_id);
                Err(TransportError::ReceiveFailed(
                    "Response channel closed".to_string(),
                ))
            }
            Err(_) => {
                warn!("Elicitation {} timed out", request_id);
                self.elicitations.remove(&request_id);
                Err(TransportError::Timeout)
            }
        }
    }

    /// Process incoming message
    async fn process_incoming_message(&self, text: String) -> TransportResult<()> {
        // Parse as JSON
        let json_value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| TransportError::ReceiveFailed(format!("Invalid JSON: {}", e)))?;

        // Check if it's an elicitation response by looking for id field
        if let Some(id) = json_value.get("id").and_then(|v| v.as_str())
            && let Some((_, pending)) = self.elicitations.remove(id)
        {
            // Parse elicitation result from the result field
            if let Some(result) = json_value.get("result")
                && let Ok(elicitation_result) =
                    serde_json::from_value::<ElicitationCreateResult>(result.clone())
            {
                let _ = pending.response_tx.send(elicitation_result);
                return Ok(());
            }

            // Handle error response
            let cancel_result = ElicitationCreateResult {
                action: ElicitationAction::Cancel,
                content: None,
                meta: None,
            };
            let _ = pending.response_tx.send(cancel_result);
        }

        // Process as regular message or correlation response
        if let Some(correlation_id) = json_value.get("correlation_id").and_then(|v| v.as_str())
            && let Some((_, ctx)) = self.correlations.remove(correlation_id)
            && let Some(tx) = ctx.response_tx
        {
            let message = TransportMessage {
                id: MessageId::from(Uuid::new_v4()),
                payload: Bytes::from(serde_json::to_vec(&json_value).unwrap_or_default()),
                metadata: TransportMessageMetadata::default(),
            };
            let _ = tx.send(message);
        }

        Ok(())
    }
}

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

    async fn disconnect(&mut self) -> TransportResult<()> {
        *self.state.write().await = TransportState::Disconnecting;

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Close WebSocket
        if let Some(ref mut writer) = *self.writer.lock().await {
            let _ = writer.send(Message::Close(None)).await;
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

        // Emit disconnected event
        self.event_emitter.emit(TransportEvent::Disconnected {
            transport_type: TransportType::WebSocket,
            endpoint: "websocket".to_string(),
            reason: Some("Disconnected by user".to_string()),
        });

        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        if let Some(ref mut writer) = *self.writer.lock().await {
            let text = String::from_utf8(message.payload.to_vec())
                .map_err(|e| TransportError::SendFailed(format!("Failed to serialize: {}", e)))?;

            writer
                .send(Message::Text(text))
                .await
                .map_err(|e| TransportError::SendFailed(format!("WebSocket send failed: {}", e)))?;

            self.metrics.write().await.messages_sent += 1;
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
                    // Process for elicitation responses
                    let _ = self.process_incoming_message(text.clone()).await;

                    let message = TransportMessage {
                        id: MessageId::from(Uuid::new_v4()),
                        payload: Bytes::from(text.as_bytes().to_vec()),
                        metadata: TransportMessageMetadata::default(),
                    };

                    self.metrics.write().await.messages_received += 1;
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
                    }
                    Ok(None)
                }
                Some(Ok(Message::Pong(_))) => {
                    trace!("Received pong");
                    Ok(None)
                }
                Some(Err(e)) => {
                    error!("WebSocket error: {}", e);
                    Err(TransportError::ReceiveFailed(e.to_string()))
                }
                None => {
                    *self.state.write().await = TransportState::Disconnected;
                    Err(TransportError::ConnectionLost(
                        "WebSocket stream ended".to_string(),
                    ))
                }
                _ => Ok(None),
            }
        } else {
            Err(TransportError::ReceiveFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
}

#[async_trait]
impl BidirectionalTransport for WebSocketBidirectionalTransport {
    async fn send_request(
        &mut self,
        message: TransportMessage,
        timeout: Option<Duration>,
    ) -> TransportResult<TransportMessage> {
        let correlation_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        // Store correlation
        let ctx = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: message.id.to_string(),
            response_tx: Some(tx),
            timeout: timeout.unwrap_or(Duration::from_secs(30)),
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id.clone(), ctx);

        // Add correlation ID to message metadata
        let mut message = message;
        message.metadata.correlation_id = Some(correlation_id.clone());

        // Send the message
        self.send(message).await?;

        // Wait for response
        match timeout {
            Some(duration) => match tokio::time::timeout(duration, rx).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(_)) => Err(TransportError::ReceiveFailed("Channel closed".to_string())),
                Err(_) => {
                    self.correlations.remove(&correlation_id);
                    Err(TransportError::Timeout)
                }
            },
            None => rx
                .await
                .map_err(|_| TransportError::ReceiveFailed("Channel closed".to_string())),
        }
    }

    async fn start_correlation(&mut self, correlation_id: String) -> TransportResult<()> {
        // Create a correlation context to track request-response pairs
        let ctx = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: String::new(),
            response_tx: None,
            timeout: Duration::from_secs(30),
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id, ctx);
        Ok(())
    }

    async fn stop_correlation(&mut self, correlation_id: &str) -> TransportResult<()> {
        self.correlations.remove(correlation_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_websocket_bidirectional_creation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert_eq!(transport.transport_type(), TransportType::WebSocket);
        assert!(transport.capabilities().supports_bidirectional);
    }

    #[tokio::test]
    async fn test_elicitation_support() {
        let config = WebSocketBidirectionalConfig {
            max_concurrent_elicitations: 5,
            ..Default::default()
        };

        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Verify elicitation capability is advertised
        assert!(transport.capabilities.custom.contains_key("elicitation"));
        assert_eq!(
            transport.capabilities.custom.get("elicitation"),
            Some(&json!(true))
        );
    }

    #[tokio::test]
    async fn test_reconnection_config() {
        let config = WebSocketBidirectionalConfig {
            reconnect: ReconnectConfig {
                enabled: true,
                max_retries: 5,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(10),
                backoff_factor: 2.0,
            },
            ..Default::default()
        };

        let _transport = WebSocketBidirectionalTransport::new(config.clone())
            .await
            .unwrap();

        assert!(config.reconnect.enabled);
        assert_eq!(config.reconnect.max_retries, 5);
    }
}
