//! Standard I/O transport implementation.
//!
//! This transport uses stdin/stdout for communication, which is the
//! standard way MCP servers communicate with clients. It supports
//! JSON-RPC over newline-delimited JSON.
//!
//! # MCP Specification Compliance (2025-06-18)
//!
//! This implementation is **fully compliant** with the MCP stdio transport specification:
//!
//! - ✅ **Newline-delimited JSON**: Uses `LinesCodec` for proper message framing
//! - ✅ **No embedded newlines**: Validates messages don't contain `\n` or `\r` characters
//! - ✅ **UTF-8 encoding**: All messages are UTF-8 encoded (enforced by `std::str::from_utf8`)
//! - ✅ **stderr for logging**: Uses `tracing` crate which outputs to stderr by default
//! - ✅ **Bidirectional communication**: Supports both client→server and server→client messages
//! - ✅ **Valid JSON only**: Validates all messages are well-formed JSON before sending
//!
//! Per MCP spec: "Messages are delimited by newlines, and **MUST NOT** contain embedded newlines."
//!
//! # Interior Mutability Pattern
//!
//! This transport follows the research-backed hybrid mutex pattern for
//! optimal performance in async contexts:
//!
//! - **std::sync::Mutex** for state/config (short-lived locks, never cross .await)
//! - **AtomicMetrics** for lock-free counter updates (10-100x faster than Mutex)
//! - **tokio::sync::Mutex** for I/O streams (only when necessary, cross .await points)

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use serde_json;
use tokio::io::{BufReader, Stdin, Stdout};
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{debug, error, trace, warn};
use turbomcp_protocol::MessageId;
use uuid::Uuid;

use crate::core::{
    AtomicMetrics, Transport, TransportCapabilities, TransportConfig, TransportError,
    TransportEventEmitter, TransportFactory, TransportMessage, TransportMessageMetadata,
    TransportMetrics, TransportResult, TransportState, TransportType,
};

// Type alias to reduce complexity for clippy
type StdinReader = FramedRead<BufReader<Stdin>, LinesCodec>;
type StdoutWriter = FramedWrite<Stdout, LinesCodec>;

/// Standard I/O transport implementation
///
/// # Interior Mutability Architecture
///
/// Following research-backed 2025 Rust async best practices:
///
/// - `state`: std::sync::Mutex (short-lived locks, never held across .await)
/// - `config`: std::sync::Mutex (infrequent updates, short-lived locks)
/// - `metrics`: AtomicMetrics (lock-free counters, 10-100x faster than Mutex)
/// - I/O streams: tokio::sync::Mutex (held across .await, necessary for async I/O)
#[derive(Debug)]
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

    /// Stdin reader (tokio::sync::Mutex - crosses await boundaries)
    stdin_reader: Arc<TokioMutex<Option<StdinReader>>>,

    /// Stdout writer (tokio::sync::Mutex - crosses await boundaries)
    stdout_writer: Arc<TokioMutex<Option<StdoutWriter>>>,

    /// Message receive channel (tokio::sync::Mutex - crosses await boundaries)
    receive_channel: Arc<TokioMutex<Option<mpsc::Receiver<TransportMessage>>>>,

    /// Background task handle (tokio::sync::Mutex - crosses await boundaries)
    _task_handle: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl StdioTransport {
    /// Create a new stdio transport
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
            stdin_reader: Arc::new(TokioMutex::new(None)),
            stdout_writer: Arc::new(TokioMutex::new(None)),
            receive_channel: Arc::new(TokioMutex::new(None)),
            _task_handle: Arc::new(TokioMutex::new(None)),
        }
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
        // Setup stdin reader
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut stdin_reader = FramedRead::new(reader, LinesCodec::new());

        // Setup stdout writer
        let stdout = tokio::io::stdout();
        *self.stdout_writer.lock().await = Some(FramedWrite::new(stdout, LinesCodec::new()));

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

                            if let Err(e) = crate::core::validate_response_size(size, &limits) {
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
        crate::core::validate_request_size(size, &config.limits)?;

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
    // use serde_json::json;
    // use tokio_test;

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
        // - ✅ ALLOWED: JSON with ESCAPED newlines like {"text":"line1\nline2"}
        //   The \n here is TWO bytes: backslash (0x5C) + 'n' (0x6E)
        //   This does NOT contain a literal newline byte (0x0A)
        //
        // - ❌ FORBIDDEN: JSON with LITERAL newline bytes like {"text":"line1<0x0A>line2"}
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
}
