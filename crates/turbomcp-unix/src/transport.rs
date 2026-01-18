//! Unix domain socket transport implementation for MCP

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use turbomcp_protocol::MessageId;
use turbomcp_transport_traits::{
    AtomicMetrics, Transport, TransportCapabilities, TransportError, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};

/// Unix domain socket transport implementation with integrated security
#[derive(Debug)]
pub struct UnixTransport {
    /// Socket path
    socket_path: PathBuf,
    /// Server mode flag
    is_server: bool,
    /// Message sender for incoming messages (tokio mutex - crosses await)
    sender: Arc<tokio::sync::Mutex<Option<mpsc::Sender<TransportMessage>>>>,
    /// Message receiver for incoming messages (tokio mutex - crosses await)
    receiver: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<TransportMessage>>>>,
    /// Active connections map: path -> outgoing message sender (std mutex - short-lived)
    connections: Arc<StdMutex<HashMap<String, mpsc::Sender<String>>>>,
    /// Transport capabilities (immutable)
    capabilities: TransportCapabilities,
    /// Current state (std mutex - short-lived)
    state: Arc<StdMutex<TransportState>>,
    /// Transport metrics (lock-free atomic)
    metrics: Arc<AtomicMetrics>,
}

impl UnixTransport {
    /// Create a new Unix socket transport for server mode
    #[must_use]
    pub fn new_server(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            is_server: true,
            sender: Arc::new(tokio::sync::Mutex::new(None)),
            receiver: Arc::new(tokio::sync::Mutex::new(None)),
            connections: Arc::new(StdMutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            metrics: Arc::new(AtomicMetrics::default()),
        }
    }

    /// Create a new Unix socket transport for client mode
    #[must_use]
    pub fn new_client(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            is_server: false,
            sender: Arc::new(tokio::sync::Mutex::new(None)),
            receiver: Arc::new(tokio::sync::Mutex::new(None)),
            connections: Arc::new(StdMutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            metrics: Arc::new(AtomicMetrics::default()),
        }
    }

    /// Create a new Unix socket transport for client mode with security validation
    /// Start Unix socket server
    async fn start_server(&self) -> TransportResult<()> {
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
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connecting;

        let listener = UnixListener::bind(&self.socket_path).map_err(|e| {
            *self.state.lock().expect("state mutex poisoned") = TransportState::Failed {
                reason: format!("Failed to bind: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to bind Unix socket listener: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        *self.sender.lock().await = Some(tx.clone());
        *self.receiver.lock().await = Some(rx);
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connected;

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
    async fn connect_client(&self) -> TransportResult<()> {
        info!("Connecting to Unix socket at {:?}", self.socket_path);
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connecting;

        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            *self.state.lock().expect("state mutex poisoned") = TransportState::Failed {
                reason: format!("Failed to connect: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to connect to Unix socket: {e}"))
        })?;

        // Create channels for bidirectional communication (same pattern as TCP)
        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        *self.sender.lock().await = Some(tx.clone());
        *self.receiver.lock().await = Some(rx);
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connected;

        // Handle connection using the same framed approach as TCP and server connections
        // This ensures the client gets registered in the connections HashMap
        let incoming_sender = tx.clone();
        let connections = self.connections.clone();

        // Use oneshot channel to wait for connection registration
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            if let Err(e) = handle_unix_connection_framed_with_signal(
                stream,
                incoming_sender,
                connections,
                ready_tx,
            )
            .await
            {
                error!("Unix client connection handler failed: {}", e);
            }
        });

        // Wait for the connection to be registered before returning
        // This prevents race conditions where send() is called before
        // the connection is added to the HashMap
        ready_rx.await.map_err(|_| {
            TransportError::ConnectionFailed("Connection registration failed".into())
        })?;

        info!("Successfully connected to Unix socket server");
        Ok(())
    }
}

/// Handle a Unix socket connection using tokio-util::codec::Framed with LinesCodec
/// This provides proven newline-delimited JSON framing with proper bidirectional communication
async fn handle_unix_connection_framed(
    stream: UnixStream,
    incoming_sender: mpsc::Sender<TransportMessage>,
    connections: Arc<StdMutex<HashMap<String, mpsc::Sender<String>>>>,
) -> TransportResult<()> {
    handle_unix_connection_framed_with_signal(stream, incoming_sender, connections, None).await
}

/// Handle a Unix socket connection with optional ready signal
/// The ready_tx channel is sent when the connection is registered, allowing callers to wait
async fn handle_unix_connection_framed_with_signal(
    stream: UnixStream,
    incoming_sender: mpsc::Sender<TransportMessage>,
    connections: Arc<StdMutex<HashMap<String, mpsc::Sender<String>>>>,
    ready_tx: impl Into<Option<tokio::sync::oneshot::Sender<()>>>,
) -> TransportResult<()> {
    let ready_tx = ready_tx.into();
    debug!("Handling Unix socket connection using Framed<UnixStream, LinesCodec>");

    // Create framed transport using LinesCodec for newline-delimited messages
    let framed = Framed::new(stream, LinesCodec::new());
    let (mut sink, mut stream) = framed.split();

    // Channel for outgoing messages to this specific connection (bounded for backpressure)
    let (outgoing_sender, mut outgoing_receiver) = mpsc::channel::<String>(100);

    // Register this connection in the connections map
    // Generate unique key for each connection to avoid overwrites
    let connection_key = format!("unix-conn-{}", Uuid::new_v4());
    debug!(
        "Registering Unix socket connection with key: {}",
        connection_key
    );
    connections
        .lock()
        .expect("connections mutex poisoned")
        .insert(connection_key.clone(), outgoing_sender);
    debug!(
        "Total connections now: {}",
        connections
            .lock()
            .expect("connections mutex poisoned")
            .len()
    );

    // Signal that the connection is ready (for client mode)
    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }

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
                let max_size = turbomcp_protocol::MAX_MESSAGE_SIZE;
                if line.len() > max_size {
                    error!(
                        "Message size {} exceeds limit {} from Unix socket",
                        line.len(),
                        max_size
                    );
                    break;
                }

                debug!("Received message from Unix socket: {}", line);

                // Parse and validate JSON-RPC message
                match serde_json::from_str::<serde_json::Value>(&line) {
                    Ok(value) => {
                        // Extract message ID for transport tracking
                        let id = value.get("id").cloned().unwrap_or_else(|| {
                            serde_json::Value::String(Uuid::new_v4().to_string())
                        });

                        let message_id = match id {
                            serde_json::Value::String(s) => MessageId::from(s),
                            serde_json::Value::Number(n) => {
                                MessageId::from(n.as_i64().unwrap_or_default())
                            }
                            _ => MessageId::from(Uuid::new_v4()),
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
    connections_cleanup
        .lock()
        .expect("connections mutex poisoned")
        .remove(&cleanup_key);
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
        self.state.lock().expect("state mutex poisoned").clone()
    }

    async fn connect(&self) -> TransportResult<()> {
        if self.is_server {
            self.start_server().await
        } else {
            self.connect_client().await
        }
    }

    async fn disconnect(&self) -> TransportResult<()> {
        info!("Stopping Unix socket transport");
        *self.state.lock().expect("state mutex poisoned") = TransportState::Disconnecting;
        *self.sender.lock().await = None;
        *self.receiver.lock().await = None;

        // Clean up socket file if we're the server (ASYNC - Non-blocking!)
        if self.is_server
            && self.socket_path.exists()
            && let Err(e) = tokio::fs::remove_file(&self.socket_path).await
        {
            debug!("Failed to remove socket file: {}", e);
        }

        *self.state.lock().expect("state mutex poisoned") = TransportState::Disconnected;
        Ok(())
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_sent
            .fetch_add(message.size() as u64, Ordering::Relaxed);

        // Use unified channel-based approach for both server and client (same as TCP transport)
        let json_str = String::from_utf8_lossy(&message.payload).to_string();
        let connections = self.connections.lock().expect("connections mutex poisoned");
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
            let mut connections = self.connections.lock().expect("connections mutex poisoned");
            for key in failed_connections {
                connections.remove(&key);
            }
        }

        Ok(())
    }

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        // Use unified channel-based reception for both server and client (same as TCP transport)
        let mut receiver_guard = self.receiver.lock().await;
        if let Some(ref mut receiver) = *receiver_guard {
            match receiver.recv().await {
                Some(message) => {
                    self.metrics
                        .messages_received
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .bytes_received
                        .fetch_add(message.size() as u64, Ordering::Relaxed);
                    Ok(Some(message))
                }
                None => {
                    *self.state.lock().expect("state mutex poisoned") = TransportState::Failed {
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
        self.metrics.snapshot()
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
        assert!(matches!(
            *transport.state.lock().expect("state mutex poisoned"),
            TransportState::Disconnected
        ));
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
