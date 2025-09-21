//! Standard I/O transport implementation.
//!
//! This transport uses stdin/stdout for communication, which is the
//! standard way MCP servers communicate with clients. It supports
//! JSON-RPC over newline-delimited JSON.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json;
use tokio::io::{BufReader, Stdin, Stdout};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{debug, error, trace, warn};
use turbomcp_core::MessageId;
use uuid::Uuid;

use crate::core::{
    Transport, TransportCapabilities, TransportConfig, TransportError, TransportEventEmitter,
    TransportFactory, TransportMessage, TransportMessageMetadata, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

/// Standard I/O transport implementation
#[derive(Debug)]
pub struct StdioTransport {
    /// Transport state
    state: Arc<Mutex<TransportState>>,

    /// Transport capabilities
    capabilities: TransportCapabilities,

    /// Transport configuration
    config: TransportConfig,

    /// Metrics collector
    metrics: Arc<Mutex<TransportMetrics>>,

    /// Event emitter
    event_emitter: TransportEventEmitter,

    /// Stdin reader
    stdin_reader: Option<FramedRead<BufReader<Stdin>, LinesCodec>>,

    /// Stdout writer
    stdout_writer: Option<FramedWrite<Stdout, LinesCodec>>,

    /// Message receive channel (bounded for backpressure)
    receive_channel: Option<mpsc::Receiver<TransportMessage>>,

    /// Background task handle
    _task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl StdioTransport {
    /// Create a new stdio transport
    #[must_use]
    pub fn new() -> Self {
        let (event_emitter, _) = TransportEventEmitter::new();

        Self {
            state: Arc::new(Mutex::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: std::collections::HashMap::new(),
            },
            config: TransportConfig {
                transport_type: TransportType::Stdio,
                ..Default::default()
            },
            metrics: Arc::new(Mutex::new(TransportMetrics::default())),
            event_emitter,
            stdin_reader: None,
            stdout_writer: None,
            receive_channel: None,
            _task_handle: None,
        }
    }

    /// Create a stdio transport with custom configuration
    #[must_use]
    pub fn with_config(config: TransportConfig) -> Self {
        let mut transport = Self::new();
        transport.config = config;
        transport
    }

    /// Create a stdio transport with event emitter
    #[must_use]
    pub fn with_event_emitter(event_emitter: TransportEventEmitter) -> Self {
        let mut transport = Self::new();
        transport.event_emitter = event_emitter;
        transport
    }

    fn update_metrics<F>(&self, updater: F)
    where
        F: FnOnce(&mut TransportMetrics),
    {
        let mut metrics = self.metrics.lock();
        updater(&mut metrics);
    }

    fn set_state(&self, new_state: TransportState) {
        let mut state = self.state.lock();
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
        // Update metrics via message counters; no dedicated heartbeat counter
        self.update_metrics(|m| m.messages_sent = m.messages_sent.saturating_add(0));
    }

    async fn setup_stdio_streams(&mut self) -> TransportResult<()> {
        // Setup stdin reader
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        self.stdin_reader = Some(FramedRead::new(reader, LinesCodec::new()));

        // Setup stdout writer
        let stdout = tokio::io::stdout();
        self.stdout_writer = Some(FramedWrite::new(stdout, LinesCodec::new()));

        // Setup message receive channel (bounded for backpressure)
        let (tx, rx) = mpsc::channel(1000);
        self.receive_channel = Some(rx);

        // Start background reader task
        if let Some(mut reader) = self.stdin_reader.take() {
            let sender = tx;
            let event_emitter = self.event_emitter.clone();
            let metrics = self.metrics.clone();

            let task_handle = tokio::spawn(async move {
                while let Some(result) = reader.next().await {
                    match result {
                        Ok(line) => {
                            trace!("Received line: {}", line);

                            match Self::parse_message(&line) {
                                Ok(message) => {
                                    let size = message.size();

                                    // Update metrics
                                    {
                                        let mut m = metrics.lock();
                                        m.messages_received += 1;
                                        m.bytes_received += size as u64;
                                    }

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

            self._task_handle = Some(task_handle);
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
        self.state.lock().clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        if matches!(self.state().await, TransportState::Connected) {
            return Ok(());
        }

        self.set_state(TransportState::Connecting);

        match self.setup_stdio_streams().await {
            Ok(()) => {
                self.update_metrics(|m| m.connections += 1);
                self.set_state(TransportState::Connected);
                debug!("Stdio transport connected");
                Ok(())
            }
            Err(e) => {
                self.update_metrics(|m| m.failed_connections += 1);
                self.set_state(TransportState::Failed {
                    reason: e.to_string(),
                });
                error!("Failed to connect stdio transport: {}", e);
                Err(e)
            }
        }
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        if matches!(self.state().await, TransportState::Disconnected) {
            return Ok(());
        }

        self.set_state(TransportState::Disconnecting);

        // Close streams
        self.stdin_reader = None;
        self.stdout_writer = None;
        self.receive_channel = None;

        // Cancel background task
        if let Some(handle) = self._task_handle.take() {
            handle.abort();
        }

        self.set_state(TransportState::Disconnected);
        debug!("Stdio transport disconnected");
        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        let state = self.state().await;
        if !matches!(state, TransportState::Connected) {
            return Err(TransportError::ConnectionFailed(format!(
                "Transport not connected: {state}"
            )));
        }

        let json_line = Self::serialize_message(&message)?;
        let size = json_line.len();

        if let Some(writer) = &mut self.stdout_writer {
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

            // Update metrics
            self.update_metrics(|m| {
                m.messages_sent += 1;
                m.bytes_sent += size as u64;
            });

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

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        let state = self.state().await;
        if !matches!(state, TransportState::Connected) {
            return Err(TransportError::ConnectionFailed(format!(
                "Transport not connected: {state}"
            )));
        }

        if let Some(receiver) = &mut self.receive_channel {
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
        self.metrics.lock().clone()
    }

    fn endpoint(&self) -> Option<String> {
        Some("stdio://".to_string())
    }

    async fn configure(&mut self, config: TransportConfig) -> TransportResult<()> {
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

        self.config = config;
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
        assert_eq!(transport.config.connect_timeout, Duration::from_secs(10));
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
        let mut transport = StdioTransport::new();

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
