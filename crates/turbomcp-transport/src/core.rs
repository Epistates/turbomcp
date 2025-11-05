//! Core transport traits, types, and errors.
//!
//! This module defines the fundamental abstractions for sending and receiving MCP messages
//! over different communication protocols. The central piece is the [`Transport`] trait,
//! which provides a generic interface for all transport implementations.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Sink, Stream};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use turbomcp_protocol::MessageId;

/// A specialized `Result` type for transport operations.
pub type TransportResult<T> = std::result::Result<T, TransportError>;

/// Represents errors that can occur during transport operations.
#[derive(Error, Debug, Clone)]
pub enum TransportError {
    /// Failed to establish a connection.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// An established connection was lost.
    #[error("Connection lost: {0}")]
    ConnectionLost(String),

    /// Failed to send a message.
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Failed to receive a message.
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    /// Failed to serialize or deserialize a message.
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// A protocol-level error occurred.
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// The operation did not complete within the specified timeout.
    #[error("Operation timed out")]
    Timeout,

    /// Connection establishment timed out.
    ///
    /// This error occurs when the TCP/TLS handshake takes too long to complete.
    /// Consider checking network connectivity or increasing the connect timeout.
    #[error(
        "Connection timed out after {timeout:?} for operation: {operation}. \
         If this is expected, increase the timeout with \
         `TimeoutConfig {{ connect: Duration::from_secs({}) }}`",
        timeout.as_secs() * 2
    )]
    ConnectionTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// Single request timed out.
    ///
    /// This error occurs when a single request-response cycle takes too long.
    /// For slow operations like LLM sampling, consider using `TimeoutConfig::patient()`.
    #[error(
        "Request timed out after {timeout:?} for operation: {operation}. \
         If this is expected, increase the timeout with \
         `TimeoutConfig {{ request: Some(Duration::from_secs({})) }}` \
         or use `TimeoutConfig::patient()` for slow operations",
        timeout.as_secs() * 2
    )]
    RequestTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// Total operation timed out (including retries).
    ///
    /// This error occurs when the entire operation, including all retries,
    /// takes too long. This timeout is broader than request timeout.
    #[error(
        "Total operation timed out after {timeout:?} for operation: {operation}. \
         This includes retries. If this is expected, increase the timeout with \
         `TimeoutConfig {{ total: Some(Duration::from_secs({})) }}`",
        timeout.as_secs() * 2
    )]
    TotalTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// Read operation timed out (streaming).
    ///
    /// This error occurs when reading a chunk from a streaming response takes too long.
    /// For slow streaming operations, consider using `TimeoutConfig::patient()`.
    #[error(
        "Read timed out after {timeout:?} while streaming response for operation: {operation}. \
         If this is expected, increase the timeout with \
         `TimeoutConfig {{ read: Some(Duration::from_secs({})) }}`",
        timeout.as_secs() * 2
    )]
    ReadTimeout {
        /// The operation that timed out
        operation: String,
        /// The timeout duration that was exceeded
        timeout: Duration,
    },

    /// The transport was configured with invalid parameters.
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Authentication with the remote endpoint failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// The request was rejected due to rate limiting.
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// The requested transport is not available.
    #[error("Transport not available: {0}")]
    NotAvailable(String),

    /// An underlying I/O error occurred.
    #[error("IO error: {0}")]
    Io(String),

    /// An unexpected internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Request size exceeds the configured maximum limit.
    ///
    /// This error protects against memory exhaustion attacks by rejecting
    /// requests that are too large. If you need to send larger requests,
    /// increase the limit with `LimitsConfig::max_request_size`.
    #[error(
        "Request size ({size} bytes) exceeds maximum allowed ({max} bytes). \
         If this is expected, increase the limit with \
         `LimitsConfig {{ max_request_size: Some({}) }}` or use `LimitsConfig::unlimited()` \
         if running behind an API gateway.",
        size
    )]
    RequestTooLarge {
        /// The actual size of the request in bytes
        size: usize,
        /// The maximum allowed size in bytes
        max: usize,
    },

    /// Response size exceeds the configured maximum limit.
    ///
    /// This error protects against memory exhaustion attacks by rejecting
    /// responses that are too large. If you need to receive larger responses,
    /// increase the limit with `LimitsConfig::max_response_size`.
    #[error(
        "Response size ({size} bytes) exceeds maximum allowed ({max} bytes). \
         If this is expected, increase the limit with \
         `LimitsConfig {{ max_response_size: Some({}) }}` or use `LimitsConfig::unlimited()` \
         if running behind an API gateway.",
        size
    )]
    ResponseTooLarge {
        /// The actual size of the response in bytes
        size: usize,
        /// The maximum allowed size in bytes
        max: usize,
    },
}

/// Enumerates the types of transports supported by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Standard Input/Output, for command-line servers.
    Stdio,
    /// HTTP, including Server-Sent Events (SSE).
    Http,
    /// WebSocket for full-duplex communication.
    WebSocket,
    /// TCP sockets for network communication.
    Tcp,
    /// Unix domain sockets for local inter-process communication.
    Unix,
    /// A transport that manages a child process.
    ChildProcess,
    /// gRPC for high-performance RPC.
    #[cfg(feature = "grpc")]
    Grpc,
    /// QUIC for a modern, multiplexed transport.
    #[cfg(feature = "quic")]
    Quic,
}

/// Represents the current state of a transport connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportState {
    /// The transport is not connected.
    Disconnected,
    /// The transport is in the process of connecting.
    Connecting,
    /// The transport is connected and ready to send/receive messages.
    Connected,
    /// The transport is in the process of disconnecting.
    Disconnecting,
    /// The transport has encountered an unrecoverable error.
    Failed {
        /// A description of the failure reason.
        reason: String,
    },
}

/// Describes the capabilities of a transport implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportCapabilities {
    /// The maximum message size in bytes that the transport can handle.
    pub max_message_size: Option<usize>,

    /// Whether the transport supports message compression.
    pub supports_compression: bool,

    /// Whether the transport supports streaming data.
    pub supports_streaming: bool,

    /// Whether the transport supports full-duplex bidirectional communication.
    pub supports_bidirectional: bool,

    /// Whether the transport can handle multiple concurrent requests over a single connection.
    pub supports_multiplexing: bool,

    /// A list of supported compression algorithms.
    pub compression_algorithms: Vec<String>,

    /// A map for any other custom capabilities.
    pub custom: HashMap<String, serde_json::Value>,
}

/// Configuration for a transport instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// The type of the transport.
    pub transport_type: TransportType,

    /// The maximum time to wait for a connection to be established.
    pub connect_timeout: Duration,

    /// The maximum time to wait for a read operation to complete.
    pub read_timeout: Option<Duration>,

    /// The maximum time to wait for a write operation to complete.
    pub write_timeout: Option<Duration>,

    /// The interval for sending keep-alive messages to maintain the connection.
    pub keep_alive: Option<Duration>,

    /// The maximum number of concurrent connections allowed.
    pub max_connections: Option<usize>,

    /// Whether to enable message compression.
    pub compression: bool,

    /// The preferred compression algorithm to use.
    pub compression_algorithm: Option<String>,

    /// Size limits for requests and responses (v2.2.0+).
    ///
    /// By default, enforces 10MB response limit and 1MB request limit
    /// to prevent memory exhaustion attacks.
    #[serde(default)]
    pub limits: crate::config::LimitsConfig,

    /// Timeout configuration for operations (v2.2.0+).
    ///
    /// By default, enforces balanced timeouts (30s connect, 60s request, 120s total)
    /// to prevent hanging requests and resource exhaustion.
    #[serde(default)]
    pub timeouts: crate::config::TimeoutConfig,

    /// TLS/HTTPS configuration (v2.2.0+).
    ///
    /// By default, uses TLS 1.2 for backward compatibility in v2.2.0.
    /// Use `TlsConfig::modern()` for TLS 1.3 (recommended).
    ///
    /// This configuration applies to HTTP and WebSocket transports.
    #[serde(default)]
    pub tls: crate::config::TlsConfig,

    /// A map for any other custom configuration.
    pub custom: HashMap<String, serde_json::Value>,
}

/// A wrapper for a message being sent or received over a transport.
#[derive(Debug, Clone)]
pub struct TransportMessage {
    /// The unique identifier of the message.
    pub id: MessageId,

    /// The binary payload of the message.
    pub payload: Bytes,

    /// Metadata associated with the message.
    pub metadata: TransportMessageMetadata,
}

/// Metadata associated with a `TransportMessage`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMessageMetadata {
    /// The encoding of the message payload (e.g., "gzip").
    pub encoding: Option<String>,

    /// The MIME type of the message payload (e.g., "application/json").
    pub content_type: Option<String>,

    /// An ID used to correlate requests and responses.
    pub correlation_id: Option<String>,

    /// A map of custom headers.
    pub headers: HashMap<String, String>,

    /// The priority of the message (higher numbers indicate higher priority).
    pub priority: Option<u8>,

    /// The time-to-live for the message, in milliseconds.
    pub ttl: Option<u64>,

    /// A marker indicating that this is a heartbeat message.
    pub is_heartbeat: Option<bool>,
}

/// A serializable snapshot of a transport's performance metrics.
///
/// This struct provides a consistent view of metrics for external monitoring.
/// For internal, high-performance updates, `AtomicMetrics` is preferred.
///
/// # Custom Transport Metrics
/// Transport implementations can store custom metrics in the `metadata` field.
/// ```
/// # use turbomcp_transport::core::TransportMetrics;
/// # use serde_json::json;
/// let mut metrics = TransportMetrics::default();
/// metrics.metadata.insert("active_correlations".to_string(), json!(42));
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMetrics {
    /// Total number of bytes sent.
    pub bytes_sent: u64,

    /// Total number of bytes received.
    pub bytes_received: u64,

    /// Total number of messages sent.
    pub messages_sent: u64,

    /// Total number of messages received.
    pub messages_received: u64,

    /// Total number of connection attempts.
    pub connections: u64,

    /// Total number of failed connection attempts.
    pub failed_connections: u64,

    /// The average latency of operations, in milliseconds.
    pub average_latency_ms: f64,

    /// The current number of active connections.
    pub active_connections: u64,

    /// The compression ratio (uncompressed size / compressed size), if applicable.
    pub compression_ratio: Option<f64>,

    /// A map for custom, transport-specific metrics.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// A lock-free, atomic structure for high-performance metrics updates.
///
/// This struct uses `AtomicU64` for all counters, which is significantly faster
/// than using mutexes for simple counter updates.
///
/// # Performance
/// - Lock-free increments and decrements.
/// - No contention on updates.
/// - Uses `Ordering::Relaxed` for maximum performance where strict ordering is not required.
#[derive(Debug)]
pub struct AtomicMetrics {
    /// Total bytes sent (atomic counter).
    pub bytes_sent: std::sync::atomic::AtomicU64,

    /// Total bytes received (atomic counter).
    pub bytes_received: std::sync::atomic::AtomicU64,

    /// Total messages sent (atomic counter).
    pub messages_sent: std::sync::atomic::AtomicU64,

    /// Total messages received (atomic counter).
    pub messages_received: std::sync::atomic::AtomicU64,

    /// Total connection attempts (atomic counter).
    pub connections: std::sync::atomic::AtomicU64,

    /// Failed connection attempts (atomic counter).
    pub failed_connections: std::sync::atomic::AtomicU64,

    /// Current active connections (atomic counter).
    pub active_connections: std::sync::atomic::AtomicU64,

    /// The average latency, stored as an exponential moving average in microseconds.
    avg_latency_us: std::sync::atomic::AtomicU64,

    /// Total bytes before compression.
    uncompressed_bytes: std::sync::atomic::AtomicU64,

    /// Total bytes after compression.
    compressed_bytes: std::sync::atomic::AtomicU64,
}

impl Default for AtomicMetrics {
    fn default() -> Self {
        use std::sync::atomic::AtomicU64;
        Self {
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            failed_connections: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            avg_latency_us: AtomicU64::new(0),
            uncompressed_bytes: AtomicU64::new(0),
            compressed_bytes: AtomicU64::new(0),
        }
    }
}

impl AtomicMetrics {
    /// Creates a new `AtomicMetrics` instance with all counters initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the average latency using an exponential moving average (EMA).
    ///
    /// This method uses an EMA with alpha = 0.1 for smooth latency tracking.
    ///
    /// # Arguments
    /// * `latency_us` - The new latency measurement in microseconds.
    pub fn update_latency_us(&self, latency_us: u64) {
        use std::sync::atomic::Ordering;

        let current = self.avg_latency_us.load(Ordering::Relaxed);
        let new_avg = if current == 0 {
            latency_us
        } else {
            // EMA with alpha = 0.1: new_avg = old_avg * 0.9 + new_value * 0.1
            (current * 9 + latency_us) / 10
        };
        self.avg_latency_us.store(new_avg, Ordering::Relaxed);
    }

    /// Records compression statistics to track the compression ratio.
    ///
    /// # Arguments
    /// * `uncompressed_size` - The size of the data before compression.
    /// * `compressed_size` - The size of the data after compression.
    pub fn record_compression(&self, uncompressed_size: u64, compressed_size: u64) {
        use std::sync::atomic::Ordering;

        self.uncompressed_bytes
            .fetch_add(uncompressed_size, Ordering::Relaxed);
        self.compressed_bytes
            .fetch_add(compressed_size, Ordering::Relaxed);
    }

    /// Creates a serializable `TransportMetrics` snapshot from the current atomic values.
    ///
    /// This method uses `Ordering::Relaxed` for maximum performance.
    pub fn snapshot(&self) -> TransportMetrics {
        use std::sync::atomic::Ordering;

        let avg_latency_us = self.avg_latency_us.load(Ordering::Relaxed);
        let uncompressed = self.uncompressed_bytes.load(Ordering::Relaxed);
        let compressed = self.compressed_bytes.load(Ordering::Relaxed);

        let compression_ratio = if compressed > 0 && uncompressed > 0 {
            Some(uncompressed as f64 / compressed as f64)
        } else {
            None
        };

        TransportMetrics {
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            connections: self.connections.load(Ordering::Relaxed),
            failed_connections: self.failed_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            average_latency_ms: (avg_latency_us as f64) / 1000.0, // Convert μs to ms
            compression_ratio,
            metadata: HashMap::new(), // Empty metadata for base atomic metrics
        }
    }

    /// Resets all atomic metric counters to zero.
    pub fn reset(&self) {
        use std::sync::atomic::Ordering;

        self.bytes_sent.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.messages_sent.store(0, Ordering::Relaxed);
        self.messages_received.store(0, Ordering::Relaxed);
        self.connections.store(0, Ordering::Relaxed);
        self.failed_connections.store(0, Ordering::Relaxed);
        self.active_connections.store(0, Ordering::Relaxed);
        self.avg_latency_us.store(0, Ordering::Relaxed);
        self.uncompressed_bytes.store(0, Ordering::Relaxed);
        self.compressed_bytes.store(0, Ordering::Relaxed);
    }
}

/// Represents events that occur within a transport's lifecycle.
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// A new connection has been established.
    Connected {
        /// The type of the transport that connected.
        transport_type: TransportType,
        /// The endpoint of the connection.
        endpoint: String,
    },

    /// A connection has been lost.
    Disconnected {
        /// The type of the transport that disconnected.
        transport_type: TransportType,
        /// The endpoint of the connection.
        endpoint: String,
        /// An optional reason for the disconnection.
        reason: Option<String>,
    },

    /// A message has been successfully sent.
    MessageSent {
        /// The ID of the sent message.
        message_id: MessageId,
        /// The size of the sent message in bytes.
        size: usize,
    },

    /// A message has been successfully received.
    MessageReceived {
        /// The ID of the received message.
        message_id: MessageId,
        /// The size of the received message in bytes.
        size: usize,
    },

    /// An error has occurred in the transport.
    Error {
        /// The error that occurred.
        error: TransportError,
        /// Optional additional context about the error.
        context: Option<String>,
    },

    /// The transport's metrics have been updated.
    MetricsUpdated {
        /// The updated metrics snapshot.
        metrics: TransportMetrics,
    },
}

/// The core trait for all transport implementations.
///
/// This trait defines the essential, asynchronous operations for a message-based
/// communication channel, such as connecting, disconnecting, sending, and receiving.
#[async_trait]
pub trait Transport: Send + Sync + std::fmt::Debug {
    /// Returns the type of this transport.
    fn transport_type(&self) -> TransportType;

    /// Returns the capabilities of this transport.
    fn capabilities(&self) -> &TransportCapabilities;

    /// Returns the current state of the transport.
    async fn state(&self) -> TransportState;

    /// Establishes a connection to the remote endpoint.
    async fn connect(&self) -> TransportResult<()>;

    /// Closes the connection to the remote endpoint.
    async fn disconnect(&self) -> TransportResult<()>;

    /// Sends a single message over the transport.
    async fn send(&self, message: TransportMessage) -> TransportResult<()>;

    /// Receives a single message from the transport in a non-blocking way.
    async fn receive(&self) -> TransportResult<Option<TransportMessage>>;

    /// Returns a snapshot of the transport's current performance metrics.
    async fn metrics(&self) -> TransportMetrics;

    /// Returns `true` if the transport is currently in the `Connected` state.
    async fn is_connected(&self) -> bool {
        matches!(self.state().await, TransportState::Connected)
    }

    /// Returns the endpoint address or identifier for this transport, if applicable.
    fn endpoint(&self) -> Option<String> {
        None
    }

    /// Applies a new configuration to the transport.
    async fn configure(&self, config: TransportConfig) -> TransportResult<()> {
        // Default implementation does nothing. Transports can override this.
        let _ = config;
        Ok(())
    }
}

/// A trait for transports that support full-duplex, bidirectional communication.
///
/// This extends the base `Transport` trait with the ability to send a request and
/// await a correlated response.
#[async_trait]
pub trait BidirectionalTransport: Transport {
    /// Sends a request message and waits for a corresponding response.
    async fn send_request(
        &self,
        message: TransportMessage,
        timeout: Option<Duration>,
    ) -> TransportResult<TransportMessage>;

    /// Starts tracking a request-response correlation.
    async fn start_correlation(&self, correlation_id: String) -> TransportResult<()>;

    /// Stops tracking a request-response correlation.
    async fn stop_correlation(&self, correlation_id: &str) -> TransportResult<()>;
}

/// A trait for transports that support streaming data.
#[async_trait]
pub trait StreamingTransport: Transport {
    /// The type of the stream used for sending messages.
    type SendStream: Stream<Item = TransportResult<TransportMessage>> + Send + Unpin;

    /// The type of the sink used for receiving messages.
    type ReceiveStream: Sink<TransportMessage, Error = TransportError> + Send + Unpin;

    /// Returns a stream for sending messages.
    async fn send_stream(&self) -> TransportResult<Self::SendStream>;

    /// Returns a sink for receiving messages.
    async fn receive_stream(&self) -> TransportResult<Self::ReceiveStream>;
}

/// A factory for creating instances of a specific transport type.
pub trait TransportFactory: Send + Sync + std::fmt::Debug {
    /// Returns the type of transport this factory creates.
    fn transport_type(&self) -> TransportType;

    /// Creates a new transport instance with the given configuration.
    fn create(&self, config: TransportConfig) -> TransportResult<Box<dyn Transport>>;

    /// Returns `true` if this transport is available on the current system.
    fn is_available(&self) -> bool {
        true
    }
}

/// An emitter for broadcasting `TransportEvent`s to listeners.
#[derive(Debug, Clone)]
pub struct TransportEventEmitter {
    sender: mpsc::Sender<TransportEvent>,
}

impl TransportEventEmitter {
    /// Creates a new event emitter and a corresponding receiver.
    #[must_use]
    pub fn new() -> (Self, mpsc::Receiver<TransportEvent>) {
        let (sender, receiver) = mpsc::channel(500); // Bounded channel for backpressure
        (Self { sender }, receiver)
    }

    /// Emits an event, dropping it if the channel is full to avoid blocking.
    pub fn emit(&self, event: TransportEvent) {
        // Use try_send for non-blocking event emission.
        if self.sender.try_send(event).is_err() {
            // Ignore the error if the channel is full or closed.
        }
    }

    /// Emits a `Connected` event.
    pub fn emit_connected(&self, transport_type: TransportType, endpoint: String) {
        self.emit(TransportEvent::Connected {
            transport_type,
            endpoint,
        });
    }

    /// Emits a `Disconnected` event.
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

    /// Emits a `MessageSent` event.
    pub fn emit_message_sent(&self, message_id: MessageId, size: usize) {
        self.emit(TransportEvent::MessageSent { message_id, size });
    }

    /// Emits a `MessageReceived` event.
    pub fn emit_message_received(&self, message_id: MessageId, size: usize) {
        self.emit(TransportEvent::MessageReceived { message_id, size });
    }

    /// Emits an `Error` event.
    pub fn emit_error(&self, error: TransportError, context: Option<String>) {
        self.emit(TransportEvent::Error { error, context });
    }

    /// Emits a `MetricsUpdated` event.
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
            max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
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
            limits: crate::config::LimitsConfig::default(),
            timeouts: crate::config::TimeoutConfig::default(),
            tls: crate::config::TlsConfig::default(),
            custom: HashMap::new(),
        }
    }
}

impl TransportMessage {
    /// Creates a new `TransportMessage` with a given ID and payload.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_transport::core::TransportMessage;
    /// # use turbomcp_protocol::MessageId;
    /// # use bytes::Bytes;
    /// let msg = TransportMessage::new(MessageId::from(1), Bytes::from("hello"));
    /// ```
    pub fn new(id: MessageId, payload: Bytes) -> Self {
        Self {
            id,
            payload,
            metadata: TransportMessageMetadata::default(),
        }
    }

    /// Creates a new `TransportMessage` with the given ID, payload, and metadata.
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

    /// Returns the size of the message payload in bytes.
    pub const fn size(&self) -> usize {
        self.payload.len()
    }

    /// Returns `true` if the message is compressed.
    pub const fn is_compressed(&self) -> bool {
        self.metadata.encoding.is_some()
    }

    /// Returns the content type of the message, if specified.
    pub fn content_type(&self) -> Option<&str> {
        self.metadata.content_type.as_deref()
    }

    /// Returns the correlation ID of the message, if specified.
    pub fn correlation_id(&self) -> Option<&str> {
        self.metadata.correlation_id.as_deref()
    }
}

impl TransportMessageMetadata {
    /// Creates a new `TransportMessageMetadata` with a specified content type.
    pub fn with_content_type(content_type: impl Into<String>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..Default::default()
        }
    }

    /// Creates a new `TransportMessageMetadata` with a specified correlation ID.
    pub fn with_correlation_id(correlation_id: impl Into<String>) -> Self {
        Self {
            correlation_id: Some(correlation_id.into()),
            ..Default::default()
        }
    }

    /// Adds a header to the metadata using a builder pattern.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_transport::core::TransportMessageMetadata;
    /// let metadata = TransportMessageMetadata::default()
    ///     .with_header("X-Request-ID", "123");
    /// ```
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Sets the priority of the message.
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Sets the time-to-live for the message.
    #[must_use]
    pub const fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl.as_millis() as u64);
        self
    }

    /// Marks the message as a heartbeat.
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

/// Validates that a request message size does not exceed the configured limit.
///
/// # Arguments
///
/// * `size` - The size of the request payload in bytes
/// * `limits` - The limits configuration to check against
///
/// # Returns
///
/// `Ok(())` if the size is within limits or no limit is set, otherwise `Err(TransportError::RequestTooLarge)`
///
/// # Example
///
/// ```
/// use turbomcp_transport::core::validate_request_size;
/// use turbomcp_transport::config::LimitsConfig;
///
/// let limits = LimitsConfig::default();
/// assert!(validate_request_size(1000, &limits).is_ok());
/// assert!(validate_request_size(10 * 1024 * 1024, &limits).is_err());
/// ```
pub fn validate_request_size(
    size: usize,
    limits: &crate::config::LimitsConfig,
) -> TransportResult<()> {
    if let Some(max_size) = limits.max_request_size
        && size > max_size
    {
        return Err(TransportError::RequestTooLarge {
            size,
            max: max_size,
        });
    }
    Ok(())
}

/// Validates that a response message size does not exceed the configured limit.
///
/// # Arguments
///
/// * `size` - The size of the response payload in bytes
/// * `limits` - The limits configuration to check against
///
/// # Returns
///
/// `Ok(())` if the size is within limits or no limit is set, otherwise `Err(TransportError::ResponseTooLarge)`
///
/// # Example
///
/// ```
/// use turbomcp_transport::core::validate_response_size;
/// use turbomcp_transport::config::LimitsConfig;
///
/// let limits = LimitsConfig::default();
/// assert!(validate_response_size(1000, &limits).is_ok());
/// assert!(validate_response_size(50 * 1024 * 1024, &limits).is_err());
/// ```
pub fn validate_response_size(
    size: usize,
    limits: &crate::config::LimitsConfig,
) -> TransportResult<()> {
    if let Some(max_size) = limits.max_response_size
        && size > max_size
    {
        return Err(TransportError::ResponseTooLarge {
            size,
            max: max_size,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // use std::sync::Arc;
    // use tokio_test;

    #[test]
    fn test_transport_capabilities_default() {
        let caps = TransportCapabilities::default();
        assert_eq!(
            caps.max_message_size,
            Some(turbomcp_protocol::MAX_MESSAGE_SIZE)
        );
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
