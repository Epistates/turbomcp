//! Unix domain socket transport implementation for MCP

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{debug, error, info, warn};
use uuid;

#[cfg(feature = "security")]
use turbomcp_security::{FileSecurityValidator, SecurityError, SecurityResult};

use crate::core::{
    Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
    TransportResult, TransportState, TransportType,
};
use turbomcp_core::MessageId;

/// Unix domain socket transport implementation with integrated security
#[derive(Debug)]
pub struct UnixTransport {
    /// Socket path
    socket_path: PathBuf,
    /// Server mode flag
    is_server: bool,
    /// Message sender for incoming messages (bounded for backpressure)
    sender: Option<mpsc::Sender<TransportMessage>>,
    /// Message receiver for incoming messages (bounded for backpressure)
    receiver: Option<mpsc::Receiver<TransportMessage>>,
    /// Active connections map: path -> outgoing message sender (bounded for backpressure)
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
    /// Transport capabilities
    capabilities: TransportCapabilities,
    /// Current state
    state: TransportState,
    /// Transport metrics
    metrics: TransportMetrics,
    /// Security validator for socket paths
    #[cfg(feature = "security")]
    security_validator: Option<Arc<FileSecurityValidator>>,
}

impl UnixTransport {
    /// Create a new Unix socket transport for server mode
    #[must_use]
    pub fn new_server(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            is_server: true,
            sender: None,
            receiver: None,
            connections: Arc::new(Mutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: TransportState::Disconnected,
            metrics: TransportMetrics::default(),
            #[cfg(feature = "security")]
            security_validator: None,
        }
    }

    /// Create a new Unix socket transport for server mode with security validation
    #[cfg(feature = "security")]
    #[must_use]
    pub fn new_server_secure(socket_path: PathBuf, validator: Arc<FileSecurityValidator>) -> Self {
        Self {
            socket_path,
            is_server: true,
            sender: None,
            receiver: None,
            connections: Arc::new(Mutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: TransportState::Disconnected,
            metrics: TransportMetrics::default(),
            security_validator: Some(validator),
        }
    }

    /// Create a new Unix socket transport for client mode
    #[must_use]
    pub fn new_client(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            is_server: false,
            sender: None,
            receiver: None,
            connections: Arc::new(Mutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: TransportState::Disconnected,
            metrics: TransportMetrics::default(),
            #[cfg(feature = "security")]
            security_validator: None,
        }
    }

    /// Create a new Unix socket transport for client mode with security validation
    #[cfg(feature = "security")]
    #[must_use]
    pub fn new_client_secure(socket_path: PathBuf, validator: Arc<FileSecurityValidator>) -> Self {
        Self {
            socket_path,
            is_server: false,
            sender: None,
            receiver: None,
            connections: Arc::new(Mutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: TransportState::Disconnected,
            metrics: TransportMetrics::default(),
            security_validator: Some(validator),
        }
    }

    /// Start Unix socket server
    async fn start_server(&mut self) -> TransportResult<()> {
        // Validate socket path through security layer if available
        #[cfg(feature = "security")]
        if let Some(ref validator) = self.security_validator {
            let validated_path = validator
                .validate_socket_path(&self.socket_path)
                .await
                .map_err(|e| {
                    TransportError::ConfigurationError(format!(
                        "Socket path security validation failed: {e}"
                    ))
                })?;

            // Update socket path with validated path
            self.socket_path = validated_path;
        }

        // Remove existing socket file if it exists (ASYNC - Non-blocking!)
        if self.socket_path.exists() {
            tokio::fs::remove_file(&self.socket_path)
                .await
                .map_err(|e| {
                    TransportError::ConfigurationError(format!(
                        "Failed to remove existing socket file: {e}"
                    ))
                })?;
        }

        info!("Starting Unix socket server at {:?}", self.socket_path);
        self.state = TransportState::Connecting;

        let listener = UnixListener::bind(&self.socket_path).map_err(|e| {
            self.state = TransportState::Failed {
                reason: format!("Failed to bind: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to bind Unix socket listener: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        self.sender = Some(tx.clone());
        self.receiver = Some(rx);
        self.state = TransportState::Connected;

        // Accept connections in background
        let connections = self.connections.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        info!("Accepted Unix socket connection");
                        let incoming_sender = tx.clone();
                        let connections_ref = connections.clone();
                        // Handle connection in separate task
                        tokio::spawn(async move {
                            if let Err(e) = handle_unix_connection_framed(
                                stream,
                                incoming_sender,
                                connections_ref,
                            )
                            .await
                            {
                                error!("Unix socket connection handler failed: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Unix socket connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Connect to Unix socket server using standard practices
    /// Following the proven TCP transport pattern for consistent architecture
    async fn connect_client(&mut self) -> TransportResult<()> {
        // Validate socket path through security layer if available
        #[cfg(feature = "security")]
        if let Some(ref validator) = self.security_validator {
            let validated_path = validator
                .validate_socket_path(&self.socket_path)
                .await
                .map_err(|e| {
                    TransportError::ConfigurationError(format!(
                        "Socket path security validation failed: {e}"
                    ))
                })?;

            // Update socket path with validated path
            self.socket_path = validated_path;
        }

        info!("Connecting to Unix socket at {:?}", self.socket_path);
        self.state = TransportState::Connecting;

        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            self.state = TransportState::Failed {
                reason: format!("Failed to connect: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to connect to Unix socket: {e}"))
        })?;

        // Create channels for bidirectional communication (same pattern as TCP)
        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        self.sender = Some(tx.clone());
        self.receiver = Some(rx);
        self.state = TransportState::Connected;

        // Handle connection using the same framed approach as TCP and server connections
        // This ensures the client gets registered in the connections HashMap
        let incoming_sender = tx.clone();
        let connections = self.connections.clone();
        tokio::spawn(async move {
            if let Err(e) =
                handle_unix_connection_framed(stream, incoming_sender, connections).await
            {
                error!("Unix client connection handler failed: {}", e);
            }
        });

        info!("Successfully connected to Unix socket server");
        Ok(())
    }
}

/// Handle a Unix socket connection using tokio-util::codec::Framed with LinesCodec
/// This provides production-grade newline-delimited JSON framing with proper bidirectional communication
async fn handle_unix_connection_framed(
    stream: UnixStream,
    incoming_sender: mpsc::Sender<TransportMessage>,
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
) -> TransportResult<()> {
    debug!("Handling Unix socket connection using Framed<UnixStream, LinesCodec>");

    // Create framed transport using LinesCodec for newline-delimited messages
    let framed = Framed::new(stream, LinesCodec::new());
    let (mut sink, mut stream) = framed.split();

    // Channel for outgoing messages to this specific connection (bounded for backpressure)
    let (outgoing_sender, mut outgoing_receiver) = mpsc::channel::<String>(100);

    // Register this connection in the connections map
    // Generate unique key for each connection to avoid overwrites
    let connection_key = format!("unix-conn-{}", uuid::Uuid::new_v4());
    debug!(
        "Registering Unix socket connection with key: {}",
        connection_key
    );
    connections
        .lock()
        .insert(connection_key.clone(), outgoing_sender);
    debug!("Total connections now: {}", connections.lock().len());

    // Clone for cleanup
    let connections_cleanup = connections.clone();
    let cleanup_key = connection_key.clone();

    // Spawn task to handle outgoing messages (responses from server to client)
    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_receiver.recv().await {
            debug!("Sending message to Unix socket: {}", message);

            if let Err(e) = sink.send(message).await {
                error!("Failed to send message to Unix socket connection: {}", e);
                break;
            }
        }
        debug!("Unix socket send handler finished");
    });

    // Handle incoming messages using StreamExt
    while let Some(result) = stream.next().await {
        match result {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }

                // Validate message size (1MB limit for security)
                if let Err(e) = crate::security::validate_message_size(
                    line.as_bytes(),
                    turbomcp_core::MAX_MESSAGE_SIZE,
                ) {
                    error!("Message size validation failed from Unix socket: {}", e);
                    break;
                }

                debug!("Received message from Unix socket: {}", line);

                // Parse and validate JSON-RPC message
                match serde_json::from_str::<serde_json::Value>(&line) {
                    Ok(value) => {
                        // Extract message ID for transport tracking
                        let id = value.get("id").cloned().unwrap_or_else(|| {
                            serde_json::Value::String(uuid::Uuid::new_v4().to_string())
                        });

                        let message_id = match id {
                            serde_json::Value::String(s) => MessageId::from(s),
                            serde_json::Value::Number(n) => {
                                MessageId::from(n.as_i64().unwrap_or_default())
                            }
                            _ => MessageId::from(uuid::Uuid::new_v4()),
                        };

                        // Create transport message with JSON bytes
                        let transport_msg = TransportMessage::new(message_id, Bytes::from(line));

                        // Use try_send with backpressure handling
                        match incoming_sender.try_send(transport_msg) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                warn!(
                                    "Message channel full, applying backpressure to Unix socket connection"
                                );
                                // Apply backpressure by dropping this message
                                continue;
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                warn!("Message receiver dropped, closing Unix socket connection");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse JSON-RPC message from Unix socket: {}", e);
                        // Skip invalid messages but keep connection open (resilient)
                    }
                }
            }
            Err(e) => {
                error!("Failed to read from Unix socket connection: {}", e);
                break;
            }
        }
    }

    // Clean up connection
    connections_cleanup.lock().remove(&cleanup_key);
    send_task.abort();
    debug!("Unix socket connection handler finished");
    Ok(())
}

#[async_trait]
impl Transport for UnixTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Unix
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        if self.is_server {
            self.start_server().await
        } else {
            self.connect_client().await
        }
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        info!("Stopping Unix socket transport");
        self.state = TransportState::Disconnecting;
        self.sender = None;
        self.receiver = None;

        // Clean up socket file if we're the server (ASYNC - Non-blocking!)
        if self.is_server
            && self.socket_path.exists()
            && let Err(e) = tokio::fs::remove_file(&self.socket_path).await
        {
            debug!("Failed to remove socket file: {}", e);
        }

        self.state = TransportState::Disconnected;
        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        self.metrics.messages_sent += 1;
        self.metrics.bytes_sent += message.size() as u64;

        // Use unified channel-based approach for both server and client (same as TCP transport)
        let json_str = String::from_utf8_lossy(&message.payload).to_string();
        let connections = self.connections.lock();
        debug!(
            "Unix transport send: {} connections registered",
            connections.len()
        );
        for (key, _) in connections.iter() {
            debug!("  Connection key: {}", key);
        }
        if connections.is_empty() {
            return Err(TransportError::ConnectionFailed(
                "No active Unix socket connections".into(),
            ));
        }

        let mut failed_connections = Vec::new();
        for (key, sender) in connections.iter() {
            // Use try_send with backpressure handling
            match sender.try_send(json_str.clone()) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    warn!("Connection {} channel full, applying backpressure", key);
                    // Don't mark as failed, just apply backpressure
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    warn!("Failed to send message to Unix socket connection {}", key);
                    failed_connections.push(key.clone());
                }
            }
        }

        // Clean up failed connections
        drop(connections);
        if !failed_connections.is_empty() {
            let mut connections = self.connections.lock();
            for key in failed_connections {
                connections.remove(&key);
            }
        }

        Ok(())
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        // Use unified channel-based reception for both server and client (same as TCP transport)
        if let Some(ref mut receiver) = self.receiver {
            match receiver.recv().await {
                Some(message) => {
                    self.metrics.messages_received += 1;
                    self.metrics.bytes_received += message.size() as u64;
                    Ok(Some(message))
                }
                None => {
                    self.state = TransportState::Failed {
                        reason: "Channel disconnected".into(),
                    };
                    Err(TransportError::ReceiveFailed(
                        "Unix socket transport channel closed".into(),
                    ))
                }
            }
        } else {
            Err(TransportError::ConnectionFailed(
                "Unix socket transport not connected".into(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.clone()
    }

    fn endpoint(&self) -> Option<String> {
        Some(format!("unix://{}", self.socket_path.display()))
    }
}

/// Unix socket transport configuration
#[derive(Debug, Clone)]
pub struct UnixConfig {
    /// Socket file path
    pub socket_path: PathBuf,
    /// File permissions for the socket
    pub permissions: Option<u32>,
    /// Buffer size
    pub buffer_size: usize,
    /// Cleanup socket file on disconnect
    pub cleanup_on_disconnect: bool,
}

impl Default for UnixConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/tmp/turbomcp.sock"),
            permissions: Some(0o600), // Owner read/write only
            buffer_size: 8192,
            cleanup_on_disconnect: true,
        }
    }
}

/// Unix socket transport builder
#[derive(Debug)]
pub struct UnixTransportBuilder {
    config: UnixConfig,
    is_server: bool,
}

impl UnixTransportBuilder {
    /// Create a new Unix socket transport builder for server mode
    #[must_use]
    pub fn new_server() -> Self {
        Self {
            config: UnixConfig::default(),
            is_server: true,
        }
    }

    /// Create a new Unix socket transport builder for client mode
    #[must_use]
    pub fn new_client() -> Self {
        Self {
            config: UnixConfig::default(),
            is_server: false,
        }
    }

    /// Set socket path
    pub fn socket_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.config.socket_path = path.into();
        self
    }

    /// Set file permissions
    #[must_use]
    pub const fn permissions(mut self, permissions: u32) -> Self {
        self.config.permissions = Some(permissions);
        self
    }

    /// Set buffer size
    #[must_use]
    pub const fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    /// Enable or disable socket cleanup on disconnect
    #[must_use]
    pub const fn cleanup_on_disconnect(mut self, enabled: bool) -> Self {
        self.config.cleanup_on_disconnect = enabled;
        self
    }

    /// Build the Unix socket transport
    #[must_use]
    pub fn build(self) -> UnixTransport {
        if self.is_server {
            UnixTransport::new_server(self.config.socket_path)
        } else {
            UnixTransport::new_client(self.config.socket_path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_unix_config_default() {
        let config = UnixConfig::default();
        assert_eq!(config.socket_path, Path::new("/tmp/turbomcp.sock"));
        assert_eq!(config.permissions, Some(0o600));
        assert_eq!(config.buffer_size, 8192);
        assert!(config.cleanup_on_disconnect);
    }

    #[test]
    fn test_unix_transport_builder_server() {
        let transport = UnixTransportBuilder::new_server()
            .socket_path("/tmp/test-server.sock")
            .permissions(0o644)
            .buffer_size(4096)
            .build();

        assert_eq!(transport.socket_path, Path::new("/tmp/test-server.sock"));
        assert!(transport.is_server);
        assert!(matches!(transport.state, TransportState::Disconnected));
    }

    #[test]
    fn test_unix_transport_builder_client() {
        let transport = UnixTransportBuilder::new_client()
            .socket_path("/tmp/test-client.sock")
            .build();

        assert_eq!(transport.socket_path, Path::new("/tmp/test-client.sock"));
        assert!(!transport.is_server);
    }

    #[tokio::test]
    async fn test_unix_transport_state() {
        let transport = UnixTransportBuilder::new_server().build();

        assert_eq!(transport.state().await, TransportState::Disconnected);
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_endpoint() {
        let path = PathBuf::from("/tmp/test.sock");
        let transport = UnixTransport::new_server(path.clone());

        assert_eq!(
            transport.endpoint(),
            Some(format!("unix://{}", path.display()))
        );
    }

    #[test]
    fn test_unix_config_builder_pattern() {
        let config = UnixConfig {
            socket_path: PathBuf::from("/tmp/custom.sock"),
            permissions: Some(0o755),
            buffer_size: 16384,
            cleanup_on_disconnect: false,
        };

        assert_eq!(config.socket_path, Path::new("/tmp/custom.sock"));
        assert_eq!(config.permissions, Some(0o755));
        assert_eq!(config.buffer_size, 16384);
        assert!(!config.cleanup_on_disconnect);
    }
}
