//! TCP transport implementation for MCP

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinSet;
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{debug, error, info, warn};

use turbomcp_protocol::MessageId;
use turbomcp_transport_traits::{
    AtomicMetrics, Transport, TransportCapabilities, TransportError, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};

/// TCP transport implementation
pub struct TcpTransport {
    /// Local address to bind to
    bind_addr: SocketAddr,
    /// Remote address to connect to (for client mode)
    remote_addr: Option<SocketAddr>,
    /// Message sender for incoming messages (tokio mutex - crosses await)
    sender: Arc<tokio::sync::Mutex<Option<mpsc::Sender<TransportMessage>>>>,
    /// Message receiver for incoming messages (tokio mutex - crosses await)
    receiver: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<TransportMessage>>>>,
    /// Active connections map: connection ID -> outgoing message sender (std mutex - short-lived)
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
    /// Transport capabilities (immutable)
    capabilities: TransportCapabilities,
    /// Current state (std mutex - short-lived)
    state: Arc<Mutex<TransportState>>,
    /// Transport metrics (lock-free atomic)
    metrics: Arc<AtomicMetrics>,
    /// ✅ Task lifecycle management
    task_handles: Arc<tokio::sync::Mutex<JoinSet<()>>>,
    /// ✅ Shutdown signal broadcaster
    shutdown_tx: broadcast::Sender<()>,
    /// Maximum concurrent connections (DoS prevention)
    max_connections: usize,
    /// Idle connection timeout (zombie connection prevention)
    idle_timeout: std::time::Duration,
    /// Strict mode: disconnect on invalid JSON (default: false, log and continue)
    strict_mode: bool,
}

// Manual Debug implementation since broadcast::Sender doesn't implement Debug
impl std::fmt::Debug for TcpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpTransport")
            .field("bind_addr", &self.bind_addr)
            .field("remote_addr", &self.remote_addr)
            .field("capabilities", &self.capabilities)
            .field("state", &self.state)
            .field("metrics", &self.metrics)
            .finish()
    }
}

impl TcpTransport {
    /// Create a new TCP transport for server mode
    #[must_use]
    pub fn new_server(bind_addr: SocketAddr) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            bind_addr,
            remote_addr: None,
            sender: Arc::new(tokio::sync::Mutex::new(None)),
            receiver: Arc::new(tokio::sync::Mutex::new(None)),
            connections: Arc::new(Mutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: Arc::new(Mutex::new(TransportState::Disconnected)),
            metrics: Arc::new(AtomicMetrics::default()),
            task_handles: Arc::new(tokio::sync::Mutex::new(JoinSet::new())),
            shutdown_tx,
            max_connections: 256,
            idle_timeout: std::time::Duration::from_secs(300),
            strict_mode: false,
        }
    }

    /// Create a new TCP transport for client mode
    #[must_use]
    pub fn new_client(bind_addr: SocketAddr, remote_addr: SocketAddr) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            bind_addr,
            remote_addr: Some(remote_addr),
            sender: Arc::new(tokio::sync::Mutex::new(None)),
            receiver: Arc::new(tokio::sync::Mutex::new(None)),
            connections: Arc::new(Mutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: Arc::new(Mutex::new(TransportState::Disconnected)),
            metrics: Arc::new(AtomicMetrics::default()),
            task_handles: Arc::new(tokio::sync::Mutex::new(JoinSet::new())),
            shutdown_tx,
            max_connections: 256,
            idle_timeout: std::time::Duration::from_secs(300),
            strict_mode: false,
        }
    }

    /// Start TCP server
    async fn start_server(&self) -> TransportResult<()> {
        info!("Starting TCP server on {}", self.bind_addr);
        *self.state.lock() = TransportState::Connecting;

        let listener = TcpListener::bind(self.bind_addr).await.map_err(|e| {
            *self.state.lock() = TransportState::Failed {
                reason: format!("Failed to bind TCP listener: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to bind TCP listener: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        *self.sender.lock().await = Some(tx.clone());
        *self.receiver.lock().await = Some(rx);
        *self.state.lock() = TransportState::Connected;

        // ✅ Accept connections in background with proper task tracking
        let connections = self.connections.clone();
        let task_handles = Arc::clone(&self.task_handles);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let max_connections = self.max_connections;
        let idle_timeout = self.idle_timeout;
        let strict_mode = self.strict_mode;

        // Spawn accept loop and store handle
        task_handles.lock().await.spawn(async move {
            // Inner JoinSet for connection handlers
            let mut connection_tasks = JoinSet::new();

            loop {
                tokio::select! {
                    // ✅ Listen for shutdown signal
                    _ = shutdown_rx.recv() => {
                        info!("TCP accept loop received shutdown signal");
                        break;
                    }

                    // Accept new connections
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                // Check connection limit before accepting
                                if connections.lock().len() >= max_connections {
                                    warn!("TCP server at connection limit ({}), rejecting connection from {}", max_connections, addr);
                                    drop(stream);
                                    continue;
                                }

                                info!("Accepted TCP connection from {}", addr);
                                let incoming_sender = tx.clone();
                                let connections_ref = connections.clone();

                                // Generate UUID-based connection ID (NAT-safe)
                                let conn_id = format!("tcp-{}-{}", addr, uuid::Uuid::new_v4());

                                // ✅ Handle connection in separate task and store handle
                                connection_tasks.spawn(async move {
                                    if let Err(e) = handle_tcp_connection_framed(
                                        stream,
                                        addr,
                                        conn_id,
                                        incoming_sender,
                                        connections_ref,
                                        idle_timeout,
                                        strict_mode,
                                    )
                                    .await
                                    {
                                        error!("TCP connection handler failed for {}: {}", addr, e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Failed to accept TCP connection: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            // ✅ Gracefully shutdown all connection handlers
            info!(
                "Shutting down {} active TCP connections",
                connection_tasks.len()
            );
            connection_tasks.shutdown().await;
            info!("TCP accept loop shutdown complete");
        });

        Ok(())
    }

    /// Connect to TCP server
    async fn connect_client(&self) -> TransportResult<()> {
        let remote_addr = self.remote_addr.ok_or_else(|| {
            TransportError::ConfigurationError("No remote address set for client".into())
        })?;

        info!("Connecting to TCP server at {}", remote_addr);
        *self.state.lock() = TransportState::Connecting;

        let stream = TcpStream::connect(remote_addr).await.map_err(|e| {
            *self.state.lock() = TransportState::Failed {
                reason: format!("Failed to connect: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to connect to TCP server: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        *self.sender.lock().await = Some(tx.clone());
        *self.receiver.lock().await = Some(rx);
        *self.state.lock() = TransportState::Connected;

        // Handle connection with proper task tracking
        let connections = self.connections.clone();
        let task_handles = Arc::clone(&self.task_handles);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let idle_timeout = self.idle_timeout;
        let strict_mode = self.strict_mode;

        // Generate UUID-based connection ID for client
        let conn_id = format!("tcp-client-{}-{}", remote_addr, uuid::Uuid::new_v4());

        task_handles.lock().await.spawn(async move {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("TCP client connection received shutdown signal");
                }
                result = handle_tcp_connection_framed(stream, remote_addr, conn_id, tx, connections, idle_timeout, strict_mode) => {
                    if let Err(e) = result {
                        error!("TCP client connection handler failed: {}", e);
                    }
                }
            }
        });

        Ok(())
    }
}

/// Handle a TCP connection using tokio-util::codec::Framed with LinesCodec
/// This provides proven newline-delimited JSON framing with proper bidirectional communication
async fn handle_tcp_connection_framed(
    stream: TcpStream,
    addr: SocketAddr,
    conn_id: String,
    incoming_sender: mpsc::Sender<TransportMessage>,
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
    idle_timeout: std::time::Duration,
    strict_mode: bool,
) -> TransportResult<()> {
    debug!(
        "Handling TCP connection from {} (ID: {}) using Framed<TcpStream, LinesCodec>",
        addr, conn_id
    );

    // Create framed transport using LinesCodec for newline-delimited messages
    let framed = Framed::new(stream, LinesCodec::new());
    let (mut sink, mut stream) = framed.split();

    // Channel for outgoing messages to this specific connection (bounded for backpressure)
    let (outgoing_sender, mut outgoing_receiver) = mpsc::channel::<String>(100);

    // Register this connection in the connections map with UUID-based key
    connections.lock().insert(conn_id.clone(), outgoing_sender);

    // Clone for cleanup
    let connections_cleanup = connections.clone();
    let cleanup_conn_id = conn_id.clone();

    // Spawn task to handle outgoing messages (responses from server to client)
    let send_conn_id = conn_id.clone();
    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_receiver.recv().await {
            debug!(
                "Sending message to connection {}: {}",
                send_conn_id, message
            );

            if let Err(e) = sink.send(message).await {
                error!(
                    "Failed to send message to TCP connection {}: {}",
                    send_conn_id, e
                );
                break;
            }
        }
        debug!("TCP send handler finished for {}", send_conn_id);
    });

    // Handle incoming messages using StreamExt with idle timeout
    loop {
        match tokio::time::timeout(idle_timeout, stream.next()).await {
            Ok(Some(result)) => {
                match result {
                    Ok(line) => {
                        if line.is_empty() {
                            continue;
                        }

                        // Validate message size (1MB limit for security)
                        let max_size = turbomcp_protocol::MAX_MESSAGE_SIZE;
                        if line.len() > max_size {
                            error!(
                                "Message size {} exceeds limit {} from {} (ID: {})",
                                line.len(),
                                max_size,
                                addr,
                                conn_id
                            );
                            break;
                        }

                        debug!("Received message from {} (ID: {}): {}", addr, conn_id, line);

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
                                let transport_msg =
                                    TransportMessage::new(message_id, Bytes::from(line));

                                // Use try_send with backpressure handling
                                match incoming_sender.try_send(transport_msg) {
                                    Ok(()) => {}
                                    Err(mpsc::error::TrySendError::Full(_)) => {
                                        warn!(
                                            "Message channel full, applying backpressure to connection {} (ID: {})",
                                            addr, conn_id
                                        );
                                        // Apply backpressure by dropping this message
                                        continue;
                                    }
                                    Err(mpsc::error::TrySendError::Closed(_)) => {
                                        warn!(
                                            "Message receiver dropped, closing connection to {} (ID: {})",
                                            addr, conn_id
                                        );
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Failed to parse JSON-RPC message from {} (ID: {}): {}",
                                    addr, conn_id, e
                                );
                                // In strict mode, disconnect on invalid JSON
                                if strict_mode {
                                    warn!(
                                        "Strict mode enabled: closing connection {} (ID: {}) due to invalid JSON",
                                        addr, conn_id
                                    );
                                    break;
                                }
                                // In permissive mode (default), skip invalid messages but keep connection open
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to read from TCP connection {} (ID: {}): {}",
                            addr, conn_id, e
                        );
                        break;
                    }
                }
            }
            Ok(None) => {
                // Stream ended
                debug!("TCP stream ended for {} (ID: {})", addr, conn_id);
                break;
            }
            Err(_) => {
                // Idle timeout
                warn!(
                    "TCP connection {} (ID: {}) idle for {:?}, closing",
                    addr, conn_id, idle_timeout
                );
                break;
            }
        }
    }

    // Clean up connection
    connections_cleanup.lock().remove(&cleanup_conn_id);
    send_task.abort();
    debug!(
        "TCP connection handler finished for {} (ID: {})",
        addr, conn_id
    );
    Ok(())
}

impl Transport for TcpTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
        Box::pin(async move { self.state.lock().clone() })
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            if self.remote_addr.is_some() {
                // Client mode
                self.connect_client().await
            } else {
                // Server mode
                self.start_server().await
            }
        })
    }

    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            info!("Stopping TCP transport");
            *self.state.lock() = TransportState::Disconnecting;

            // ✅ Signal all tasks to shutdown
            let _ = self.shutdown_tx.send(());

            // ✅ Wait for all tasks to complete with timeout
            let mut tasks = self.task_handles.lock().await;
            let task_count = tasks.len();

            if task_count > 0 {
                info!("Waiting for {} TCP tasks to complete", task_count);

                let shutdown_timeout = std::time::Duration::from_secs(5);
                let start = std::time::Instant::now();

                while let Some(result) = tokio::time::timeout(
                    shutdown_timeout.saturating_sub(start.elapsed()),
                    tasks.join_next(),
                )
                .await
                .ok()
                .flatten()
                {
                    if let Err(e) = result
                        && e.is_panic()
                    {
                        warn!("TCP task panicked during shutdown: {:?}", e);
                    }
                }

                // ✅ Abort remaining tasks if timeout occurred
                if !tasks.is_empty() {
                    warn!("Aborting {} TCP tasks due to timeout", tasks.len());
                    tasks.shutdown().await;
                }

                info!("All TCP tasks shutdown complete");
            }

            // Clean up resources
            *self.sender.lock().await = None;
            *self.receiver.lock().await = None;
            *self.state.lock() = TransportState::Disconnected;
            Ok(())
        })
    }

    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
            self.metrics
                .bytes_sent
                .fetch_add(message.size() as u64, Ordering::Relaxed);

            // Convert transport message back to JSON string for sending
            let json_str = String::from_utf8_lossy(&message.payload).to_string();

            // Send to all active connections (broadcast for server mode)
            // In client mode, there should be exactly one connection
            let connections = self.connections.lock();
            if connections.is_empty() {
                return Err(TransportError::ConnectionFailed(
                    "No active TCP connections".into(),
                ));
            }

            let mut failed_connections = Vec::new();
            for (conn_id, sender) in connections.iter() {
                // Use try_send with backpressure handling
                match sender.try_send(json_str.clone()) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        warn!("Connection {} channel full, applying backpressure", conn_id);
                        // Don't mark as failed, just apply backpressure
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        warn!("Failed to send message to TCP connection {}", conn_id);
                        failed_connections.push(conn_id.clone());
                    }
                }
            }

            // Clean up failed connections
            drop(connections);
            if !failed_connections.is_empty() {
                let mut connections = self.connections.lock();
                for conn_id in failed_connections {
                    connections.remove(&conn_id);
                }
            }

            Ok(())
        })
    }

    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        Box::pin(async move {
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
                        *self.state.lock() = TransportState::Failed {
                            reason: "Channel disconnected".into(),
                        };
                        Err(TransportError::ReceiveFailed(
                            "TCP transport channel closed".into(),
                        ))
                    }
                }
            } else {
                Err(TransportError::ConnectionFailed(
                    "TCP transport not connected".into(),
                ))
            }
        })
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async move { self.metrics.snapshot() })
    }

    fn endpoint(&self) -> Option<String> {
        if let Some(remote) = self.remote_addr {
            Some(format!("tcp://{remote}"))
        } else {
            Some(format!("tcp://{}", self.bind_addr))
        }
    }
}

/// TCP transport configuration
#[derive(Debug, Clone)]
pub struct TcpConfig {
    /// Bind address for server mode
    pub bind_addr: SocketAddr,
    /// Remote address for client mode
    pub remote_addr: Option<SocketAddr>,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Keep-alive settings
    pub keep_alive: bool,
    /// Buffer sizes
    pub buffer_size: usize,
    /// Maximum concurrent connections (DoS prevention)
    pub max_connections: usize,
    /// Idle connection timeout in seconds (zombie connection prevention)
    pub idle_timeout_secs: u64,
    /// Strict mode: disconnect on invalid JSON (default: false, log and continue)
    pub strict_mode: bool,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080"
                .parse()
                .expect("Default TCP bind address should be valid"),
            remote_addr: None,
            connect_timeout_ms: 5000,
            keep_alive: true,
            buffer_size: 8192,
            max_connections: 256,
            idle_timeout_secs: 300,
            strict_mode: false,
        }
    }
}

/// TCP transport builder
#[derive(Debug)]
pub struct TcpTransportBuilder {
    config: TcpConfig,
}

impl TcpTransportBuilder {
    /// Create a new TCP transport builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: TcpConfig::default(),
        }
    }

    /// Set bind address
    #[must_use]
    pub const fn bind_addr(mut self, addr: SocketAddr) -> Self {
        self.config.bind_addr = addr;
        self
    }

    /// Set remote address for client mode
    #[must_use]
    pub const fn remote_addr(mut self, addr: SocketAddr) -> Self {
        self.config.remote_addr = Some(addr);
        self
    }

    /// Set connection timeout
    #[must_use]
    pub const fn connect_timeout_ms(mut self, timeout: u64) -> Self {
        self.config.connect_timeout_ms = timeout;
        self
    }

    /// Enable or disable keep-alive
    #[must_use]
    pub const fn keep_alive(mut self, enabled: bool) -> Self {
        self.config.keep_alive = enabled;
        self
    }

    /// Set buffer size
    #[must_use]
    pub const fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    /// Set maximum concurrent connections (server mode only)
    #[must_use]
    pub const fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set idle connection timeout in seconds
    #[must_use]
    pub const fn idle_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.config.idle_timeout_secs = timeout_secs;
        self
    }

    /// Enable strict mode: disconnect on invalid JSON (default: false, log and continue)
    #[must_use]
    pub const fn strict_mode(mut self, enabled: bool) -> Self {
        self.config.strict_mode = enabled;
        self
    }

    /// Build the TCP transport
    #[must_use]
    pub fn build(self) -> TcpTransport {
        let mut transport = if let Some(remote_addr) = self.config.remote_addr {
            TcpTransport::new_client(self.config.bind_addr, remote_addr)
        } else {
            TcpTransport::new_server(self.config.bind_addr)
        };

        transport.max_connections = self.config.max_connections;
        transport.idle_timeout = std::time::Duration::from_secs(self.config.idle_timeout_secs);
        transport.strict_mode = self.config.strict_mode;
        transport
    }
}

impl Default for TcpTransportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_config_default() {
        let config = TcpConfig::default();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:8080");
        assert_eq!(config.connect_timeout_ms, 5000);
        assert!(config.keep_alive);
    }

    #[test]
    fn test_tcp_transport_builder() {
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let transport = TcpTransportBuilder::new()
            .bind_addr(addr)
            .connect_timeout_ms(10000)
            .buffer_size(4096)
            .build();

        assert_eq!(transport.bind_addr, addr);
        assert_eq!(transport.remote_addr, None);
        assert!(matches!(
            *transport.state.lock(),
            TransportState::Disconnected
        ));
    }

    #[test]
    fn test_tcp_transport_client() {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let remote_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

        let transport = TcpTransportBuilder::new()
            .bind_addr(bind_addr)
            .remote_addr(remote_addr)
            .build();

        assert_eq!(transport.remote_addr, Some(remote_addr));
    }

    #[tokio::test]
    async fn test_tcp_transport_state() {
        let transport = TcpTransportBuilder::new().build();

        assert_eq!(transport.state().await, TransportState::Disconnected);
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }
}
