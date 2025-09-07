//! Core transport traits and types.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Sink, Stream};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use turbomcp_core::MessageId;

/// Result type for transport operations
pub type TransportResult<T> = std::result::Result<T, TransportError>;

/// Errors that can occur in transport operations
#[derive(Error, Debug, Clone)]
pub enum TransportError {
    /// Connection failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Connection lost
    #[error("Connection lost: {0}")]
    ConnectionLost(String),

    /// Send operation failed
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Receive operation failed
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    /// Serialization error
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// Protocol error
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Timeout error
    #[error("Operation timed out")]
    Timeout,

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Transport not available
    #[error("Transport not available: {0}")]
    NotAvailable(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Transport types supported by the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Standard I/O transport
    Stdio,
    /// HTTP transport (including SSE)
    Http,
    /// WebSocket transport
    WebSocket,
    /// TCP socket transport
    Tcp,
    /// Unix domain socket transport
    Unix,
    /// Child process transport
    ChildProcess,
    /// gRPC transport
    #[cfg(feature = "grpc")]
    Grpc,
    /// QUIC transport
    #[cfg(feature = "quic")]
    Quic,
}

/// Transport state information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportState {
    /// Transport is disconnected
    Disconnected,
    /// Transport is connecting
    Connecting,
    /// Transport is connected and ready
    Connected,
    /// Transport is disconnecting
    Disconnecting,
    /// Transport has failed
    Failed {
        /// Failure reason description
        reason: String,
    },
}

/// Transport capabilities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportCapabilities {
    /// Maximum message size supported
    pub max_message_size: Option<usize>,

    /// Whether compression is supported
    pub supports_compression: bool,

    /// Whether streaming is supported
    pub supports_streaming: bool,

    /// Whether bidirectional communication is supported
    pub supports_bidirectional: bool,

    /// Whether multiplexing is supported
    pub supports_multiplexing: bool,

    /// Supported compression algorithms
    pub compression_algorithms: Vec<String>,

    /// Custom capabilities
    pub custom: HashMap<String, serde_json::Value>,
}

/// Transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Transport type
    pub transport_type: TransportType,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Read timeout
    pub read_timeout: Option<Duration>,

    /// Write timeout
    pub write_timeout: Option<Duration>,

    /// Keep-alive interval
    pub keep_alive: Option<Duration>,

    /// Maximum concurrent connections
    pub max_connections: Option<usize>,

    /// Enable compression
    pub compression: bool,

    /// Compression algorithm preference
    pub compression_algorithm: Option<String>,

    /// Custom configuration
    pub custom: HashMap<String, serde_json::Value>,
}

/// Transport message wrapper
#[derive(Debug, Clone)]
pub struct TransportMessage {
    /// Message ID
    pub id: MessageId,

    /// Message payload
    pub payload: Bytes,

    /// Message metadata
    pub metadata: TransportMessageMetadata,
}

/// Transport message metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMessageMetadata {
    /// Content encoding
    pub encoding: Option<String>,

    /// Content type
    pub content_type: Option<String>,

    /// Correlation ID for request tracking
    pub correlation_id: Option<String>,

    /// Custom headers
    pub headers: HashMap<String, String>,

    /// Priority (higher numbers = higher priority)
    pub priority: Option<u8>,

    /// Time-to-live in milliseconds
    pub ttl: Option<u64>,

    /// Heartbeat marker
    pub is_heartbeat: Option<bool>,
}

/// Transport metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMetrics {
    /// Total bytes sent
    pub bytes_sent: u64,

    /// Total bytes received
    pub bytes_received: u64,

    /// Total messages sent
    pub messages_sent: u64,

    /// Total messages received
    pub messages_received: u64,

    /// Connection count
    pub connections: u64,

    /// Failed connections
    pub failed_connections: u64,

    /// Average latency in milliseconds
    pub average_latency_ms: f64,

    /// Current active connections
    pub active_connections: u64,

    /// Compression ratio (if enabled)
    pub compression_ratio: Option<f64>,
}

/// Transport events
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// Connection established
    Connected {
        /// Transport type that connected
        transport_type: TransportType,
        /// Connection endpoint
        endpoint: String,
    },

    /// Connection lost
    Disconnected {
        /// Transport type that disconnected
        transport_type: TransportType,
        /// Connection endpoint
        endpoint: String,
        /// Optional disconnect reason
        reason: Option<String>,
    },

    /// Message sent
    MessageSent {
        /// Message identifier
        message_id: MessageId,
        /// Message size in bytes
        size: usize,
    },

    /// Message received
    MessageReceived {
        /// Message identifier
        message_id: MessageId,
        /// Message size in bytes
        size: usize,
    },

    /// Error occurred
    Error {
        /// Transport error that occurred
        error: TransportError,
        /// Additional error context
        context: Option<String>,
    },

    /// Metrics updated
    MetricsUpdated {
        /// Updated transport metrics
        metrics: TransportMetrics,
    },
}

/// Core transport trait
#[async_trait]
pub trait Transport: Send + Sync + std::fmt::Debug {
    /// Get transport type
    fn transport_type(&self) -> TransportType;

    /// Get transport capabilities
    fn capabilities(&self) -> &TransportCapabilities;

    /// Get current state
    async fn state(&self) -> TransportState;

    /// Connect to the transport endpoint
    async fn connect(&mut self) -> TransportResult<()>;

    /// Disconnect from the transport
    async fn disconnect(&mut self) -> TransportResult<()>;

    /// Send a message
    async fn send(&mut self, message: TransportMessage) -> TransportResult<()>;

    /// Receive a message (non-blocking)
    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>>;

    /// Get transport metrics
    async fn metrics(&self) -> TransportMetrics;

    /// Check if transport is connected
    async fn is_connected(&self) -> bool {
        matches!(self.state().await, TransportState::Connected)
    }

    /// Get endpoint information
    fn endpoint(&self) -> Option<String> {
        None
    }

    /// Set configuration
    async fn configure(&mut self, config: TransportConfig) -> TransportResult<()> {
        // Default implementation - transports can override
        let _ = config;
        Ok(())
    }
}

/// Bidirectional transport trait for full-duplex communication
#[async_trait]
pub trait BidirectionalTransport: Transport {
    /// Send a message and wait for response
    async fn send_request(
        &mut self,
        message: TransportMessage,
        timeout: Option<Duration>,
    ) -> TransportResult<TransportMessage>;

    /// Start request-response correlation
    async fn start_correlation(&mut self, correlation_id: String) -> TransportResult<()>;

    /// Stop request-response correlation
    async fn stop_correlation(&mut self, correlation_id: &str) -> TransportResult<()>;
}

/// Streaming transport trait for continuous data flow
#[async_trait]
pub trait StreamingTransport: Transport {
    /// Stream type for sending messages
    type SendStream: Stream<Item = TransportResult<TransportMessage>> + Send + Unpin;

    /// Sink type for receiving messages
    type ReceiveStream: Sink<TransportMessage, Error = TransportError> + Send + Unpin;

    /// Get the send stream
    async fn send_stream(&mut self) -> TransportResult<Self::SendStream>;

    /// Get the receive stream
    async fn receive_stream(&mut self) -> TransportResult<Self::ReceiveStream>;
}

/// Transport factory for creating transport instances
pub trait TransportFactory: Send + Sync + std::fmt::Debug {
    /// Transport type this factory creates
    fn transport_type(&self) -> TransportType;

    /// Create a new transport instance
    fn create(&self, config: TransportConfig) -> TransportResult<Box<dyn Transport>>;

    /// Check if transport is available on this system
    fn is_available(&self) -> bool {
        true
    }
}

/// Transport event listener trait
#[async_trait]
pub trait TransportEventListener: Send + Sync {
    /// Handle a transport event
    async fn on_event(&self, event: TransportEvent);
}

/// Transport event emitter
#[derive(Debug, Clone)]
pub struct TransportEventEmitter {
    sender: mpsc::UnboundedSender<TransportEvent>,
}

impl TransportEventEmitter {
    /// Create a new event emitter
    #[must_use]
    pub fn new() -> (Self, mpsc::UnboundedReceiver<TransportEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }

    /// Emit an event
    pub fn emit(&self, event: TransportEvent) {
        let _ = self.sender.send(event);
    }

    /// Emit a connection event
    pub fn emit_connected(&self, transport_type: TransportType, endpoint: String) {
        self.emit(TransportEvent::Connected {
            transport_type,
            endpoint,
        });
    }

    /// Emit a disconnection event
    pub fn emit_disconnected(
        &self,
        transport_type: TransportType,
        endpoint: String,
        reason: Option<String>,
    ) {
        self.emit(TransportEvent::Disconnected {
            transport_type,
            endpoint,
            reason,
        });
    }

    /// Emit a message sent event
    pub fn emit_message_sent(&self, message_id: MessageId, size: usize) {
        self.emit(TransportEvent::MessageSent { message_id, size });
    }

    /// Emit a message received event
    pub fn emit_message_received(&self, message_id: MessageId, size: usize) {
        self.emit(TransportEvent::MessageReceived { message_id, size });
    }

    /// Emit an error event
    pub fn emit_error(&self, error: TransportError, context: Option<String>) {
        self.emit(TransportEvent::Error { error, context });
    }

    /// Emit a metrics updated event
    pub fn emit_metrics_updated(&self, metrics: TransportMetrics) {
        self.emit(TransportEvent::MetricsUpdated { metrics });
    }
}

impl Default for TransportEventEmitter {
    fn default() -> Self {
        Self::new().0
    }
}

// Implementations for common types

impl Default for TransportCapabilities {
    fn default() -> Self {
        Self {
            max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE),
            supports_compression: false,
            supports_streaming: false,
            supports_bidirectional: true,
            supports_multiplexing: false,
            compression_algorithms: Vec::new(),
            custom: HashMap::new(),
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            transport_type: TransportType::Stdio,
            connect_timeout: Duration::from_secs(30),
            read_timeout: None,
            write_timeout: None,
            keep_alive: None,
            max_connections: None,
            compression: false,
            compression_algorithm: None,
            custom: HashMap::new(),
        }
    }
}

impl TransportMessage {
    /// Create a new transport message
    pub fn new(id: MessageId, payload: Bytes) -> Self {
        Self {
            id,
            payload,
            metadata: TransportMessageMetadata::default(),
        }
    }

    /// Create a transport message with metadata
    pub const fn with_metadata(
        id: MessageId,
        payload: Bytes,
        metadata: TransportMessageMetadata,
    ) -> Self {
        Self {
            id,
            payload,
            metadata,
        }
    }

    /// Get message size
    pub const fn size(&self) -> usize {
        self.payload.len()
    }

    /// Check if message has compression
    pub const fn is_compressed(&self) -> bool {
        self.metadata.encoding.is_some()
    }

    /// Get content type
    pub fn content_type(&self) -> Option<&str> {
        self.metadata.content_type.as_deref()
    }

    /// Get correlation ID
    pub fn correlation_id(&self) -> Option<&str> {
        self.metadata.correlation_id.as_deref()
    }
}

impl TransportMessageMetadata {
    /// Create metadata with content type
    pub fn with_content_type(content_type: impl Into<String>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..Default::default()
        }
    }

    /// Create metadata with correlation ID
    pub fn with_correlation_id(correlation_id: impl Into<String>) -> Self {
        Self {
            correlation_id: Some(correlation_id.into()),
            ..Default::default()
        }
    }

    /// Add a header
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set priority
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Set TTL
    #[must_use]
    pub const fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl.as_millis() as u64);
        self
    }

    /// Mark as heartbeat
    #[must_use]
    pub const fn heartbeat(mut self) -> Self {
        self.is_heartbeat = Some(true);
        self
    }
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio => write!(f, "stdio"),
            Self::Http => write!(f, "http"),
            Self::WebSocket => write!(f, "websocket"),
            Self::Tcp => write!(f, "tcp"),
            Self::Unix => write!(f, "unix"),
            Self::ChildProcess => write!(f, "child_process"),
            #[cfg(feature = "grpc")]
            Self::Grpc => write!(f, "grpc"),
            #[cfg(feature = "quic")]
            Self::Quic => write!(f, "quic"),
        }
    }
}

impl fmt::Display for TransportState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "disconnected"),
            Self::Connecting => write!(f, "connecting"),
            Self::Connected => write!(f, "connected"),
            Self::Disconnecting => write!(f, "disconnecting"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<serde_json::Error> for TransportError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationFailed(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use std::sync::Arc;
    // use tokio_test;

    #[test]
    fn test_transport_capabilities_default() {
        let caps = TransportCapabilities::default();
        assert_eq!(caps.max_message_size, Some(turbomcp_core::MAX_MESSAGE_SIZE));
        assert!(caps.supports_bidirectional);
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.transport_type, TransportType::Stdio);
        assert_eq!(config.connect_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_transport_message_creation() {
        let id = MessageId::from("test");
        let payload = Bytes::from("test payload");
        let msg = TransportMessage::new(id.clone(), payload.clone());

        assert_eq!(msg.id, id);
        assert_eq!(msg.payload, payload);
        assert_eq!(msg.size(), 12);
    }

    #[test]
    fn test_transport_message_metadata() {
        let metadata = TransportMessageMetadata::default()
            .with_header("custom", "value")
            .with_priority(5)
            .with_ttl(Duration::from_secs(30));

        assert_eq!(metadata.headers.get("custom"), Some(&"value".to_string()));
        assert_eq!(metadata.priority, Some(5));
        assert_eq!(metadata.ttl, Some(30000));
    }

    #[test]
    fn test_transport_types_display() {
        assert_eq!(TransportType::Stdio.to_string(), "stdio");
        assert_eq!(TransportType::Http.to_string(), "http");
        assert_eq!(TransportType::WebSocket.to_string(), "websocket");
        assert_eq!(TransportType::Tcp.to_string(), "tcp");
        assert_eq!(TransportType::Unix.to_string(), "unix");
    }

    #[test]
    fn test_transport_state_display() {
        assert_eq!(TransportState::Connected.to_string(), "connected");
        assert_eq!(TransportState::Disconnected.to_string(), "disconnected");
        assert_eq!(
            TransportState::Failed {
                reason: "timeout".to_string()
            }
            .to_string(),
            "failed: timeout"
        );
    }

    #[tokio::test]
    async fn test_transport_event_emitter() {
        let (emitter, mut receiver) = TransportEventEmitter::new();

        emitter.emit_connected(TransportType::Stdio, "stdio://".to_string());

        let event = receiver.recv().await.unwrap();
        match event {
            TransportEvent::Connected {
                transport_type,
                endpoint,
            } => {
                assert_eq!(transport_type, TransportType::Stdio);
                assert_eq!(endpoint, "stdio://");
            }
            other => {
                // Avoid panic in test to align with production error handling philosophy
                eprintln!("Unexpected event variant: {other:?}");
                assert!(
                    matches!(other, TransportEvent::Connected { .. }),
                    "Expected Connected event"
                );
            }
        }
    }
}
