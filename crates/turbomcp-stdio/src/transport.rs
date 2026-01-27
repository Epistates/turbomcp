//! Standard I/O transport implementation.
//!
//! This module provides the [`StdioTransport`] implementation for MCP communication
//! over stdin/stdout. It supports JSON-RPC over newline-delimited JSON.
//!
//! # Interior Mutability Pattern
//!
//! This transport follows the research-backed hybrid mutex pattern for
//! optimal performance in async contexts:
//!
//! - **std::sync::Mutex** for state/config (short-lived locks, never cross .await)
//! - **AtomicMetrics** for lock-free counter updates (10-100x faster than Mutex)
//! - **tokio::sync::Mutex** for I/O streams (only when necessary, cross .await points)

use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::process::Child;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{debug, error, trace, warn};
use turbomcp_protocol::MessageId;
use turbomcp_transport_traits::{
    AtomicMetrics, Transport, TransportCapabilities, TransportConfig, TransportError,
    TransportEventEmitter, TransportFactory, TransportMessage, TransportMessageMetadata,
    TransportMetrics, TransportResult, TransportState, TransportType, validate_request_size,
    validate_response_size,
};
use uuid::Uuid;

// Type aliases for boxed async I/O to support both process stdio and child stdio
type BoxedAsyncRead = Pin<Box<dyn AsyncRead + Send + Sync + 'static>>;
type BoxedAsyncBufRead = BufReader<BoxedAsyncRead>;
type BoxedAsyncWrite = Pin<Box<dyn AsyncWrite + Send + Sync + 'static>>;
type StdinReader = FramedRead<BoxedAsyncBufRead, LinesCodec>;
type StdoutWriter = FramedWrite<BoxedAsyncWrite, LinesCodec>;

/// Source of stdio streams for the transport
enum StreamSource {
    /// Use the current process's stdin/stdout
    ProcessStdio,
    /// Use raw streams (already boxed)
    Raw {
        reader: Option<BoxedAsyncRead>,
        writer: Option<BoxedAsyncWrite>,
    },
}

impl std::fmt::Debug for StreamSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProcessStdio => write!(f, "ProcessStdio"),
            Self::Raw { reader, writer } => f
                .debug_struct("Raw")
                .field("reader", &reader.as_ref().map(|_| "<async reader>"))
                .field("writer", &writer.as_ref().map(|_| "<async writer>"))
                .finish(),
        }
    }
}

/// Standard I/O transport implementation
///
/// Supports communication over:
/// - Current process stdin/stdout (default)
/// - Child process stdin/stdout (via `from_child` or `from_raw`)
///
/// # Interior Mutability Architecture
///
/// Following research-backed 2025 Rust async best practices:
///
/// - `state`: std::sync::Mutex (short-lived locks, never held across .await)
/// - `config`: std::sync::Mutex (infrequent updates, short-lived locks)
/// - `metrics`: AtomicMetrics (lock-free counters, 10-100x faster than Mutex)
/// - I/O streams: tokio::sync::Mutex (held across .await, necessary for async I/O)
///
/// # Examples
///
/// ## Using current process stdio
///
/// ```rust,ignore
/// use turbomcp_stdio::StdioTransport;
///
/// let transport = StdioTransport::new();
/// ```
///
/// ## Using a spawned child process
///
/// ```rust,ignore
/// use tokio::process::Command;
/// use turbomcp_stdio::StdioTransport;
///
/// let child = Command::new("my-mcp-server")
///     .stdin(std::process::Stdio::piped())
///     .stdout(std::process::Stdio::piped())
///     .spawn()?;
///
/// let transport = StdioTransport::from_child(child)?;
/// ```
pub struct StdioTransport {
    /// Transport state (std::sync::Mutex - never crosses await)
    state: Arc<StdMutex<TransportState>>,

    /// Transport capabilities (immutable after construction)
    capabilities: TransportCapabilities,

    /// Transport configuration (std::sync::Mutex - infrequent access)
    config: Arc<StdMutex<TransportConfig>>,

    /// Lock-free atomic metrics (10-100x faster than Mutex)
    metrics: Arc<AtomicMetrics>,

    /// Event emitter
    event_emitter: TransportEventEmitter,

    /// Source of streams (process stdio or child process)
    stream_source: Arc<TokioMutex<StreamSource>>,

    /// Stdin reader (tokio::sync::Mutex - crosses await boundaries)
    stdin_reader: Arc<TokioMutex<Option<StdinReader>>>,

    /// Stdout writer (tokio::sync::Mutex - crosses await boundaries)
    stdout_writer: Arc<TokioMutex<Option<StdoutWriter>>>,

    /// Message receive channel (tokio::sync::Mutex - crosses await boundaries)
    receive_channel: Arc<TokioMutex<Option<mpsc::Receiver<TransportMessage>>>>,

    /// Background task handle (tokio::sync::Mutex - crosses await boundaries)
    _task_handle: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl std::fmt::Debug for StdioTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioTransport")
            .field("state", &self.state)
            .field("capabilities", &self.capabilities)
            .field("config", &self.config)
            .field("metrics", &self.metrics)
            .field("stream_source", &"<StreamSource>")
            .field("stdin_reader", &"<StdinReader>")
            .field("stdout_writer", &"<StdoutWriter>")
            .field("receive_channel", &"<mpsc::Receiver>")
            .field("_task_handle", &"<JoinHandle>")
            .finish()
    }
}

impl StdioTransport {
    /// Create a new stdio transport using the current process's stdin/stdout
    #[must_use]
    pub fn new() -> Self {
        let (event_emitter, _) = TransportEventEmitter::new();

        Self {
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: std::collections::HashMap::new(),
            },
            config: Arc::new(StdMutex::new(TransportConfig {
                transport_type: TransportType::Stdio,
                ..Default::default()
            })),
            metrics: Arc::new(AtomicMetrics::default()),
            event_emitter,
            stream_source: Arc::new(TokioMutex::new(StreamSource::ProcessStdio)),
            stdin_reader: Arc::new(TokioMutex::new(None)),
            stdout_writer: Arc::new(TokioMutex::new(None)),
            receive_channel: Arc::new(TokioMutex::new(None)),
            _task_handle: Arc::new(TokioMutex::new(None)),
        }
    }

    /// Create a stdio transport from a spawned child process.
    ///
    /// This is useful for MCP clients that spawn server processes and need
    /// to communicate with them over their stdin/stdout.
    ///
    /// The child process must have been spawned with:
    /// - `stdin(Stdio::piped())`
    /// - `stdout(Stdio::piped())`
    ///
    /// # Errors
    ///
    /// Returns an error if the child's stdin or stdout was not piped.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use tokio::process::Command;
    /// use turbomcp_stdio::StdioTransport;
    ///
    /// let mut child = Command::new("my-mcp-server")
    ///     .stdin(std::process::Stdio::piped())
    ///     .stdout(std::process::Stdio::piped())
    ///     .stderr(std::process::Stdio::inherit())
    ///     .spawn()?;
    ///
    /// let transport = StdioTransport::from_child(&mut child)?;
    /// transport.connect().await?;
    ///
    /// // Communicate with the server...
    /// ```
    pub fn from_child(child: &mut Child) -> TransportResult<Self> {
        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::ConfigurationError(
                "Child process stdin was not piped. Use Stdio::piped() when spawning.".to_string(),
            )
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::ConfigurationError(
                "Child process stdout was not piped. Use Stdio::piped() when spawning.".to_string(),
            )
        })?;

        Self::from_raw(stdout, stdin)
    }

    /// Create a stdio transport from raw async read/write streams.
    ///
    /// This is a lower-level constructor that allows using any async I/O streams.
    /// For child processes, prefer using `from_child` which handles the extraction
    /// of stdin/stdout from the child process.
    ///
    /// # Arguments
    ///
    /// * `reader` - The stream to read messages from (e.g., child's stdout)
    /// * `writer` - The stream to write messages to (e.g., child's stdin)
    ///
    /// # Note on Stream Direction
    ///
    /// When communicating with a child process:
    /// - `reader` should be the child's **stdout** (what we read from)
    /// - `writer` should be the child's **stdin** (what we write to)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use tokio::process::Command;
    /// use turbomcp_stdio::StdioTransport;
    ///
    /// let mut child = Command::new("my-mcp-server")
    ///     .stdin(std::process::Stdio::piped())
    ///     .stdout(std::process::Stdio::piped())
    ///     .spawn()?;
    ///
    /// let child_stdout = child.stdout.take().unwrap();
    /// let child_stdin = child.stdin.take().unwrap();
    ///
    /// let transport = StdioTransport::from_raw(child_stdout, child_stdin)?;
    /// ```
    pub fn from_raw<R, W>(reader: R, writer: W) -> TransportResult<Self>
    where
        R: AsyncRead + Send + Sync + 'static,
        W: AsyncWrite + Send + Sync + 'static,
    {
        let (event_emitter, _) = TransportEventEmitter::new();

        let boxed_reader: BoxedAsyncRead = Box::pin(reader);
        let boxed_writer: BoxedAsyncWrite = Box::pin(writer);

        Ok(Self {
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: std::collections::HashMap::new(),
            },
            config: Arc::new(StdMutex::new(TransportConfig {
                transport_type: TransportType::Stdio,
                ..Default::default()
            })),
            metrics: Arc::new(AtomicMetrics::default()),
            event_emitter,
            stream_source: Arc::new(TokioMutex::new(StreamSource::Raw {
                reader: Some(boxed_reader),
                writer: Some(boxed_writer),
            })),
            stdin_reader: Arc::new(TokioMutex::new(None)),
            stdout_writer: Arc::new(TokioMutex::new(None)),
            receive_channel: Arc::new(TokioMutex::new(None)),
            _task_handle: Arc::new(TokioMutex::new(None)),
        })
    }

    /// Create a stdio transport with custom configuration
    #[must_use]
    pub fn with_config(config: TransportConfig) -> Self {
        let transport = Self::new();
        // std::sync::Mutex: .lock() returns LockResult, use expect() for poisoned mutex
        *transport.config.lock().expect("config mutex poisoned") = config;
        transport
    }

    /// Create a stdio transport with event emitter
    #[must_use]
    pub fn with_event_emitter(event_emitter: TransportEventEmitter) -> Self {
        let (_, _) = TransportEventEmitter::new();

        Self {
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: std::collections::HashMap::new(),
            },
            config: Arc::new(StdMutex::new(TransportConfig {
                transport_type: TransportType::Stdio,
                ..Default::default()
            })),
            metrics: Arc::new(AtomicMetrics::default()),
            event_emitter,
            stream_source: Arc::new(TokioMutex::new(StreamSource::ProcessStdio)),
            stdin_reader: Arc::new(TokioMutex::new(None)),
            stdout_writer: Arc::new(TokioMutex::new(None)),
            receive_channel: Arc::new(TokioMutex::new(None)),
            _task_handle: Arc::new(TokioMutex::new(None)),
        }
    }

    fn set_state(&self, new_state: TransportState) {
        // std::sync::Mutex: short-lived lock, never crosses await
        let mut state = self.state.lock().expect("state mutex poisoned");
        if *state != new_state {
            trace!("Stdio transport state: {:?} -> {:?}", *state, new_state);
            *state = new_state.clone();

            match new_state {
                TransportState::Connected => {
                    self.event_emitter
                        .emit_connected(TransportType::Stdio, "stdio://".to_string());
                }
                TransportState::Disconnected => {
                    self.event_emitter.emit_disconnected(
                        TransportType::Stdio,
                        "stdio://".to_string(),
                        None,
                    );
                }
                TransportState::Failed { reason } => {
                    self.event_emitter.emit_disconnected(
                        TransportType::Stdio,
                        "stdio://".to_string(),
                        Some(reason),
                    );
                }
                _ => {}
            }
        }
    }

    /// Send a ping/heartbeat to stdout to keep the connection lively (optional for stdio)
    #[allow(dead_code)]
    fn heartbeat(&self) {
        // No-op: AtomicMetrics are updated directly at send/receive sites
        // No dedicated heartbeat counter needed
    }

    async fn setup_stdio_streams(&self) -> TransportResult<()> {
        // Get the stream source and set up reader/writer accordingly
        let mut stream_source = self.stream_source.lock().await;

        let mut stdin_reader: StdinReader = match &mut *stream_source {
            StreamSource::ProcessStdio => {
                // Use current process stdio
                let stdin = tokio::io::stdin();
                let boxed_stdin: BoxedAsyncRead = Box::pin(stdin);
                let buffered_reader: BoxedAsyncBufRead = BufReader::new(boxed_stdin);
                let stdout: BoxedAsyncWrite = Box::pin(tokio::io::stdout());
                *self.stdout_writer.lock().await =
                    Some(FramedWrite::new(stdout, LinesCodec::new()));
                FramedRead::new(buffered_reader, LinesCodec::new())
            }
            StreamSource::Raw { reader, writer } => {
                // Use provided raw streams
                let raw_reader = reader.take().ok_or_else(|| {
                    TransportError::ConfigurationError(
                        "Raw reader stream already consumed".to_string(),
                    )
                })?;
                let raw_writer = writer.take().ok_or_else(|| {
                    TransportError::ConfigurationError(
                        "Raw writer stream already consumed".to_string(),
                    )
                })?;

                // Wrap the reader in a BufReader for line-based reading
                let buffered_reader: BoxedAsyncBufRead = BufReader::new(raw_reader);
                *self.stdout_writer.lock().await =
                    Some(FramedWrite::new(raw_writer, LinesCodec::new()));
                FramedRead::new(buffered_reader, LinesCodec::new())
            }
        };

        // Setup message receive channel (bounded for backpressure)
        let (tx, rx) = mpsc::channel(1000);
        *self.receive_channel.lock().await = Some(rx);

        // Start background reader task
        {
            let sender = tx;
            let event_emitter = self.event_emitter.clone();
            let metrics = self.metrics.clone();
            let config = self.config.clone();

            let task_handle = tokio::spawn(async move {
                while let Some(result) = stdin_reader.next().await {
                    match result {
                        Ok(line) => {
                            trace!("Received line: {}", line);

                            // Validate response size against configured limits (v2.2.0+)
                            let size = line.len();
                            let limits = {
                                let cfg = config.lock().expect("config mutex poisoned");
                                cfg.limits.clone()
                            };

                            if let Err(e) = validate_response_size(size, &limits) {
                                error!("Response size validation failed: {}", e);
                                event_emitter.emit_error(
                                    e.clone(),
                                    Some("response size validation".to_string()),
                                );
                                // Skip this message but continue processing
                                continue;
                            }

                            match Self::parse_message(&line) {
                                Ok(message) => {
                                    let size = message.size();

                                    // Update metrics (lock-free atomic operations)
                                    metrics.messages_received.fetch_add(1, Ordering::Relaxed);
                                    metrics
                                        .bytes_received
                                        .fetch_add(size as u64, Ordering::Relaxed);

                                    // Emit event
                                    event_emitter.emit_message_received(message.id.clone(), size);

                                    // Use try_send with backpressure handling
                                    match sender.try_send(message) {
                                        Ok(()) => {}
                                        Err(mpsc::error::TrySendError::Full(_)) => {
                                            warn!(
                                                "STDIO message channel full, applying backpressure"
                                            );
                                            // Apply backpressure by dropping this message
                                            continue;
                                        }
                                        Err(mpsc::error::TrySendError::Closed(_)) => {
                                            debug!("Receive channel closed, stopping reader task");
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse message: {}", e);
                                    event_emitter
                                        .emit_error(e, Some("message parsing".to_string()));
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to read from stdin: {}", e);
                            event_emitter.emit_error(
                                TransportError::ReceiveFailed(e.to_string()),
                                Some("stdin read".to_string()),
                            );
                            break;
                        }
                    }
                }

                debug!("Stdio reader task completed");
            });

            *self._task_handle.lock().await = Some(task_handle);
        }

        Ok(())
    }

    fn parse_message(line: &str) -> TransportResult<TransportMessage> {
        let line = line.trim();
        if line.is_empty() {
            return Err(TransportError::ProtocolError("Empty message".to_string()));
        }

        // Parse JSON
        let json_value: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| TransportError::SerializationFailed(e.to_string()))?;

        // Extract message ID
        let message_id = json_value
            .get("id")
            .and_then(|id| match id {
                serde_json::Value::String(s) => Some(MessageId::from(s.clone())),
                serde_json::Value::Number(n) => n.as_i64().map(MessageId::from),
                _ => None,
            })
            .unwrap_or_else(|| MessageId::from(Uuid::new_v4()));

        // Create transport message
        let payload = Bytes::from(line.to_string());
        let metadata = TransportMessageMetadata::with_content_type("application/json");

        Ok(TransportMessage::with_metadata(
            message_id, payload, metadata,
        ))
    }

    fn serialize_message(message: &TransportMessage) -> TransportResult<String> {
        // Convert bytes back to string for stdio transport
        let json_str = std::str::from_utf8(&message.payload)
            .map_err(|e| TransportError::SerializationFailed(e.to_string()))?;

        // MCP Spec Requirement: Messages MUST NOT contain embedded newlines
        // Per spec: "Messages are delimited by newlines, and MUST NOT contain embedded newlines"
        // This check MUST come before JSON validation to catch all newline cases
        if json_str.contains('\n') || json_str.contains('\r') {
            return Err(TransportError::ProtocolError(
                "Message contains embedded newlines (forbidden by MCP stdio specification)"
                    .to_string(),
            ));
        }

        // Validate JSON
        let _: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| TransportError::SerializationFailed(e.to_string()))?;

        Ok(json_str.to_string())
    }
}

#[async_trait]
impl Transport for StdioTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        // std::sync::Mutex: short-lived lock for reading state
        self.state.lock().expect("state mutex poisoned").clone()
    }

    async fn connect(&self) -> TransportResult<()> {
        if matches!(self.state().await, TransportState::Connected) {
            return Ok(());
        }

        self.set_state(TransportState::Connecting);

        match self.setup_stdio_streams().await {
            Ok(()) => {
                // AtomicMetrics: lock-free increment
                self.metrics.connections.fetch_add(1, Ordering::Relaxed);
                self.set_state(TransportState::Connected);
                debug!("Stdio transport connected");
                Ok(())
            }
            Err(e) => {
                // AtomicMetrics: lock-free increment
                self.metrics
                    .failed_connections
                    .fetch_add(1, Ordering::Relaxed);
                self.set_state(TransportState::Failed {
                    reason: e.to_string(),
                });
                error!("Failed to connect stdio transport: {}", e);
                Err(e)
            }
        }
    }

    async fn disconnect(&self) -> TransportResult<()> {
        if matches!(self.state().await, TransportState::Disconnected) {
            return Ok(());
        }

        self.set_state(TransportState::Disconnecting);

        // Close streams
        *self.stdin_reader.lock().await = None;
        *self.stdout_writer.lock().await = None;
        *self.receive_channel.lock().await = None;

        // Cancel background task
        if let Some(handle) = self._task_handle.lock().await.take() {
            handle.abort();
        }

        self.set_state(TransportState::Disconnected);
        debug!("Stdio transport disconnected");
        Ok(())
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        let state = self.state().await;
        if !matches!(state, TransportState::Connected) {
            return Err(TransportError::ConnectionFailed(format!(
                "Transport not connected: {state}"
            )));
        }

        let json_line = Self::serialize_message(&message)?;
        let size = json_line.len();

        // Validate request size against configured limits (v2.2.0+)
        let config = self.config.lock().expect("config mutex poisoned").clone();
        validate_request_size(size, &config.limits)?;

        let mut stdout_writer = self.stdout_writer.lock().await;
        if let Some(writer) = stdout_writer.as_mut() {
            if let Err(e) = writer.send(json_line).await {
                error!("Failed to send message: {}", e);
                self.set_state(TransportState::Failed {
                    reason: e.to_string(),
                });
                return Err(TransportError::SendFailed(e.to_string()));
            }

            // Flush to ensure message is sent immediately
            use futures::SinkExt;
            if let Err(e) = SinkExt::<String>::flush(writer).await {
                error!("Failed to flush stdout: {}", e);
                return Err(TransportError::SendFailed(e.to_string()));
            }

            // Update metrics (lock-free atomic operations)
            self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
            self.metrics
                .bytes_sent
                .fetch_add(size as u64, Ordering::Relaxed);

            // Emit event
            self.event_emitter.emit_message_sent(message.id, size);

            trace!("Sent message: {} bytes", size);
            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "Stdout writer not available".to_string(),
            ))
        }
    }

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        let state = self.state().await;
        if !matches!(state, TransportState::Connected) {
            return Err(TransportError::ConnectionFailed(format!(
                "Transport not connected: {state}"
            )));
        }

        let mut receive_channel = self.receive_channel.lock().await;
        if let Some(receiver) = receive_channel.as_mut() {
            match receiver.recv().await {
                Some(message) => {
                    trace!("Received message: {} bytes", message.size());
                    Ok(Some(message))
                }
                None => {
                    warn!("Receive channel disconnected");
                    self.set_state(TransportState::Failed {
                        reason: "Receive channel disconnected".to_string(),
                    });
                    Err(TransportError::ReceiveFailed(
                        "Channel disconnected".to_string(),
                    ))
                }
            }
        } else {
            Err(TransportError::ReceiveFailed(
                "Receive channel not available".to_string(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        // AtomicMetrics: lock-free snapshot with Ordering::Relaxed
        self.metrics.snapshot()
    }

    fn endpoint(&self) -> Option<String> {
        Some("stdio://".to_string())
    }

    async fn configure(&self, config: TransportConfig) -> TransportResult<()> {
        if config.transport_type != TransportType::Stdio {
            return Err(TransportError::ConfigurationError(format!(
                "Invalid transport type: {:?}",
                config.transport_type
            )));
        }

        // Validate configuration
        if config.connect_timeout < Duration::from_millis(100) {
            return Err(TransportError::ConfigurationError(
                "Connect timeout too small".to_string(),
            ));
        }

        // std::sync::Mutex: short-lived lock for updating config
        *self.config.lock().expect("config mutex poisoned") = config;
        debug!("Stdio transport configured");
        Ok(())
    }
}

/// Factory for creating stdio transport instances
#[derive(Debug, Default)]
pub struct StdioTransportFactory;

impl StdioTransportFactory {
    /// Create a new stdio transport factory
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TransportFactory for StdioTransportFactory {
    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn create(&self, config: TransportConfig) -> TransportResult<Box<dyn Transport>> {
        if config.transport_type != TransportType::Stdio {
            return Err(TransportError::ConfigurationError(format!(
                "Invalid transport type: {:?}",
                config.transport_type
            )));
        }

        let transport = StdioTransport::with_config(config);
        Ok(Box::new(transport))
    }

    fn is_available(&self) -> bool {
        // Stdio is always available
        true
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_stdio_transport_creation() {
        let transport = StdioTransport::new();
        assert_eq!(transport.transport_type(), TransportType::Stdio);
        assert!(transport.capabilities().supports_streaming);
        assert!(transport.capabilities().supports_bidirectional);
    }

    #[test]
    fn test_stdio_transport_with_config() {
        let config = TransportConfig {
            transport_type: TransportType::Stdio,
            connect_timeout: Duration::from_secs(10),
            ..Default::default()
        };

        let transport = StdioTransport::with_config(config);
        assert_eq!(
            transport
                .config
                .lock()
                .expect("config mutex poisoned")
                .connect_timeout,
            Duration::from_secs(10)
        );
    }

    #[tokio::test]
    async fn test_stdio_transport_state_management() {
        let transport = StdioTransport::new();
        assert_eq!(transport.state().await, TransportState::Disconnected);
    }

    #[test]
    fn test_message_parsing() {
        let json_line = r#"{"jsonrpc":"2.0","id":"test-123","method":"test","params":{}}"#;
        let message = StdioTransport::parse_message(json_line).unwrap();

        assert_eq!(message.id, MessageId::from("test-123"));
        assert_eq!(message.content_type(), Some("application/json"));
        assert!(!message.payload.is_empty());
    }

    #[test]
    fn test_message_parsing_with_numeric_id() {
        let json_line = r#"{"jsonrpc":"2.0","id":42,"method":"test","params":{}}"#;
        let message = StdioTransport::parse_message(json_line).unwrap();

        assert_eq!(message.id, MessageId::from(42));
    }

    #[test]
    fn test_message_parsing_without_id() {
        let json_line = r#"{"jsonrpc":"2.0","method":"notification","params":{}}"#;
        let message = StdioTransport::parse_message(json_line).unwrap();

        // Should generate a UUID when no ID is present
        match message.id {
            MessageId::Uuid(_) => {} // Expected
            _ => assert!(
                matches!(message.id, MessageId::Uuid(_)),
                "Expected UUID message ID"
            ),
        }
    }

    #[test]
    fn test_message_parsing_invalid_json() {
        let invalid_json = "not json at all";
        let result = StdioTransport::parse_message(invalid_json);

        assert!(matches!(
            result,
            Err(TransportError::SerializationFailed(_))
        ));
    }

    #[test]
    fn test_message_parsing_empty() {
        let result = StdioTransport::parse_message("");
        assert!(matches!(result, Err(TransportError::ProtocolError(_))));

        let result = StdioTransport::parse_message("   ");
        assert!(matches!(result, Err(TransportError::ProtocolError(_))));
    }

    #[test]
    fn test_message_serialization() {
        let json_str = r#"{"jsonrpc":"2.0","id":"test","method":"ping"}"#;
        let payload = Bytes::from(json_str);
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let serialized = StdioTransport::serialize_message(&message).unwrap();
        assert_eq!(serialized, json_str);
    }

    #[test]
    fn test_message_serialization_invalid_utf8() {
        let payload = Bytes::from(vec![0xFF, 0xFE, 0xFD]); // Invalid UTF-8
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(matches!(
            result,
            Err(TransportError::SerializationFailed(_))
        ));
    }

    #[test]
    fn test_message_serialization_invalid_json() {
        let payload = Bytes::from("not json");
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(matches!(
            result,
            Err(TransportError::SerializationFailed(_))
        ));
    }

    #[test]
    fn test_message_serialization_embedded_newline_lf() {
        // MCP Spec: Messages MUST NOT contain embedded newlines
        let json_with_newline = r#"{"jsonrpc":"2.0","id":"test","method":"test","params":{"text":"line1
line2"}}"#;
        let payload = Bytes::from(json_with_newline);
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(
            matches!(result, Err(TransportError::ProtocolError(_))),
            "Expected ProtocolError for message with LF, got: {:?}",
            result
        );
    }

    #[test]
    fn test_message_serialization_embedded_newline_crlf() {
        // MCP Spec: Messages MUST NOT contain embedded newlines (including CRLF)
        let json_with_crlf = "{\r\n\"jsonrpc\":\"2.0\",\"id\":\"test\"}";
        let payload = Bytes::from(json_with_crlf);
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(
            matches!(result, Err(TransportError::ProtocolError(_))),
            "Expected ProtocolError for message with CRLF, got: {:?}",
            result
        );
    }

    #[test]
    fn test_message_serialization_embedded_cr() {
        // MCP Spec: Messages MUST NOT contain carriage returns
        let json_with_cr = "{\r\"jsonrpc\":\"2.0\",\"id\":\"test\"}";
        let payload = Bytes::from(json_with_cr);
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(
            matches!(result, Err(TransportError::ProtocolError(_))),
            "Expected ProtocolError for message with CR, got: {:?}",
            result
        );
    }

    #[test]
    fn test_message_serialization_valid_no_newlines() {
        // Verify that valid messages without newlines are accepted
        let valid_json =
            r#"{"jsonrpc":"2.0","id":"test","method":"test","params":{"text":"single line"}}"#;
        let payload = Bytes::from(valid_json);
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(
            result.is_ok(),
            "Valid message without newlines should be accepted"
        );
        assert_eq!(result.unwrap(), valid_json);
    }

    #[test]
    fn test_message_serialization_escaped_newlines_allowed() {
        // CRITICAL TEST: This verifies the spec interpretation
        //
        // The MCP spec says: "Messages are delimited by newlines, and MUST NOT contain embedded newlines"
        //
        // This means:
        // - ALLOWED: JSON with ESCAPED newlines like {"text":"line1\nline2"}
        //   The \n here is TWO bytes: backslash (0x5C) + 'n' (0x6E)
        //   This does NOT contain a literal newline byte (0x0A)
        //
        // - FORBIDDEN: JSON with LITERAL newline bytes like {"text":"line1<0x0A>line2"}
        //   This contains the newline delimiter byte (0x0A) which breaks message framing
        //
        // This is a raw string literal (r#"..."#) so the \n is stored as two characters
        let json_with_escaped_newlines = r#"{"jsonrpc":"2.0","id":"test","method":"log","params":{"message":"line1\nline2\ntab:\there"}}"#;

        // Verify this string does NOT contain literal newline/CR bytes
        assert!(
            !json_with_escaped_newlines.contains('\n'),
            "Test setup error: raw string should not contain literal newline bytes"
        );
        assert!(
            !json_with_escaped_newlines.contains('\r'),
            "Test setup error: raw string should not contain literal CR bytes"
        );

        let payload = Bytes::from(json_with_escaped_newlines);
        let message = TransportMessage::new(MessageId::from("test"), payload);

        let result = StdioTransport::serialize_message(&message);
        assert!(
            result.is_ok(),
            "JSON with ESCAPED newlines (backslash-n) should be ALLOWED per MCP spec. Got: {:?}",
            result
        );
        assert_eq!(result.unwrap(), json_with_escaped_newlines);
    }

    #[test]
    fn test_stdio_factory() {
        let factory = StdioTransportFactory::new();
        assert_eq!(factory.transport_type(), TransportType::Stdio);
        assert!(factory.is_available());

        let config = TransportConfig {
            transport_type: TransportType::Stdio,
            ..Default::default()
        };

        let transport = factory.create(config).unwrap();
        assert_eq!(transport.transport_type(), TransportType::Stdio);
    }

    #[test]
    fn test_stdio_factory_invalid_config() {
        let factory = StdioTransportFactory::new();
        let config = TransportConfig {
            transport_type: TransportType::Http, // Wrong type
            ..Default::default()
        };

        let result = factory.create(config);
        assert!(matches!(result, Err(TransportError::ConfigurationError(_))));
    }

    #[tokio::test]
    async fn test_configuration_validation() {
        let transport = StdioTransport::new();

        // Valid configuration
        let valid_config = TransportConfig {
            transport_type: TransportType::Stdio,
            connect_timeout: Duration::from_secs(5),
            ..Default::default()
        };

        assert!(transport.configure(valid_config).await.is_ok());

        // Invalid transport type
        let invalid_config = TransportConfig {
            transport_type: TransportType::Http,
            ..Default::default()
        };

        let result = transport.configure(invalid_config).await;
        assert!(matches!(result, Err(TransportError::ConfigurationError(_))));

        // Invalid timeout
        let invalid_timeout_config = TransportConfig {
            transport_type: TransportType::Stdio,
            connect_timeout: Duration::from_millis(50), // Too small
            ..Default::default()
        };

        let result = transport.configure(invalid_timeout_config).await;
        assert!(matches!(result, Err(TransportError::ConfigurationError(_))));
    }

    #[test]
    fn test_from_raw_creation() {
        // Create mock streams using tokio's duplex
        let (client_tx, server_rx) = tokio::io::duplex(1024);
        let (server_tx, client_rx) = tokio::io::duplex(1024);

        // from_raw takes (reader, writer) - what we read from, what we write to
        let transport = StdioTransport::from_raw(server_rx, server_tx).unwrap();
        assert_eq!(transport.transport_type(), TransportType::Stdio);
        assert!(transport.capabilities().supports_streaming);
        assert!(transport.capabilities().supports_bidirectional);

        // Verify the other side can also be used
        let _client_transport = StdioTransport::from_raw(client_rx, client_tx).unwrap();
    }

    #[tokio::test]
    async fn test_from_raw_connect_and_communicate() {
        // Create mock streams using tokio's duplex for bidirectional communication
        let (client_tx, server_rx) = tokio::io::duplex(4096);
        let (server_tx, client_rx) = tokio::io::duplex(4096);

        // Server transport
        let server_transport = StdioTransport::from_raw(server_rx, server_tx).unwrap();

        // Client transport
        let client_transport = StdioTransport::from_raw(client_rx, client_tx).unwrap();

        // Both should start disconnected
        assert_eq!(server_transport.state().await, TransportState::Disconnected);
        assert_eq!(client_transport.state().await, TransportState::Disconnected);

        // Connect both
        server_transport.connect().await.unwrap();
        client_transport.connect().await.unwrap();

        assert_eq!(server_transport.state().await, TransportState::Connected);
        assert_eq!(client_transport.state().await, TransportState::Connected);

        // Disconnect both
        server_transport.disconnect().await.unwrap();
        client_transport.disconnect().await.unwrap();

        assert_eq!(server_transport.state().await, TransportState::Disconnected);
        assert_eq!(client_transport.state().await, TransportState::Disconnected);
    }

    #[test]
    fn test_stream_source_debug() {
        // Test Debug impl for StreamSource
        let process_source = StreamSource::ProcessStdio;
        let debug_str = format!("{:?}", process_source);
        assert_eq!(debug_str, "ProcessStdio");
    }
}
