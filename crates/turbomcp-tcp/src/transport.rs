//! TCP transport implementation for MCP

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
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
    /// Active connections map: addr -> outgoing message sender (std mutex - short-lived)
    connections: Arc<StdMutex<HashMap<SocketAddr, mpsc::Sender<String>>>>,
    /// Transport capabilities (immutable)
    capabilities: TransportCapabilities,
    /// Current state (std mutex - short-lived)
    state: Arc<StdMutex<TransportState>>,
    /// Transport metrics (lock-free atomic)
    metrics: Arc<AtomicMetrics>,
    /// ✅ Task lifecycle management
    task_handles: Arc<tokio::sync::Mutex<JoinSet<()>>>,
    /// ✅ Shutdown signal broadcaster
    shutdown_tx: broadcast::Sender<()>,
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
            connections: Arc::new(StdMutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            metrics: Arc::new(AtomicMetrics::default()),
            task_handles: Arc::new(tokio::sync::Mutex::new(JoinSet::new())),
            shutdown_tx,
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
            connections: Arc::new(StdMutex::new(HashMap::new())),
            capabilities: TransportCapabilities {
                supports_bidirectional: true,
                supports_streaming: true,
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE), // 1MB for security
                ..Default::default()
            },
            state: Arc::new(StdMutex::new(TransportState::Disconnected)),
            metrics: Arc::new(AtomicMetrics::default()),
            task_handles: Arc::new(tokio::sync::Mutex::new(JoinSet::new())),
            shutdown_tx,
        }
    }

    /// Start TCP server
    async fn start_server(&self) -> TransportResult<()> {
        info!("Starting TCP server on {}", self.bind_addr);
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connecting;

        let listener = TcpListener::bind(self.bind_addr).await.map_err(|e| {
            *self.state.lock().expect("state mutex poisoned") = TransportState::Failed {
                reason: format!("Failed to bind TCP listener: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to bind TCP listener: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        *self.sender.lock().await = Some(tx.clone());
        *self.receiver.lock().await = Some(rx);
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connected;

        // ✅ Accept connections in background with proper task tracking
        let connections = self.connections.clone();
        let task_handles = Arc::clone(&self.task_handles);
        let mut shutdown_rx = self.shutdown_tx.subscribe();

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
                                info!("Accepted TCP connection from {}", addr);
                                let incoming_sender = tx.clone();
                                let connections_ref = connections.clone();

                                // ✅ Handle connection in separate task and store handle
                                connection_tasks.spawn(async move {
                                    if let Err(e) = handle_tcp_connection_framed(
                                        stream,
                                        addr,
                                        incoming_sender,
                                        connections_ref,
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
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connecting;

        let stream = TcpStream::connect(remote_addr).await.map_err(|e| {
            *self.state.lock().expect("state mutex poisoned") = TransportState::Failed {
                reason: format!("Failed to connect: {e}"),
            };
            TransportError::ConnectionFailed(format!("Failed to connect to TCP server: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        *self.sender.lock().await = Some(tx.clone());
        *self.receiver.lock().await = Some(rx);
        *self.state.lock().expect("state mutex poisoned") = TransportState::Connected;

        // Handle connection
        let connections = self.connections.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_tcp_connection_framed(stream, remote_addr, tx, connections).await
            {
                error!("TCP client connection handler failed: {}", e);
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
    incoming_sender: mpsc::Sender<TransportMessage>,
    connections: Arc<StdMutex<HashMap<SocketAddr, mpsc::Sender<String>>>>,
) -> TransportResult<()> {
    debug!(
        "Handling TCP connection from {} using Framed<TcpStream, LinesCodec>",
        addr
    );

    // Create framed transport using LinesCodec for newline-delimited messages
    let framed = Framed::new(stream, LinesCodec::new());
    let (mut sink, mut stream) = framed.split();

    // Channel for outgoing messages to this specific connection (bounded for backpressure)
    let (outgoing_sender, mut outgoing_receiver) = mpsc::channel::<String>(100);

    // Register this connection in the connections map
    connections
        .lock()
        .expect("connections mutex poisoned")
        .insert(addr, outgoing_sender);

    // Clone for cleanup
    let connections_cleanup = connections.clone();

    // Spawn task to handle outgoing messages (responses from server to client)
    let send_addr = addr;
    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_receiver.recv().await {
            debug!("Sending message to {}: {}", send_addr, message);

            if let Err(e) = sink.send(message).await {
                error!(
                    "Failed to send message to TCP connection {}: {}",
                    send_addr, e
                );
                break;
            }
        }
        debug!("TCP send handler finished for {}", send_addr);
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
                        "Message size {} exceeds limit {} from {}",
                        line.len(),
                        max_size,
                        addr
                    );
                    break;
                }

                debug!("Received message from {}: {}", addr, line);

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
                                    "Message channel full, applying backpressure to connection {}",
                                    addr
                                );
                                // Apply backpressure by dropping this message
                                continue;
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                warn!("Message receiver dropped, closing connection to {}", addr);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse JSON-RPC message from {}: {}", addr, e);
                        // Skip invalid messages but keep connection open (resilient)
                    }
                }
            }
            Err(e) => {
                error!("Failed to read from TCP connection {}: {}", addr, e);
                break;
            }
        }
    }

    // Clean up connection
    connections_cleanup
        .lock()
        .expect("connections mutex poisoned")
        .remove(&addr);
    send_task.abort();
    debug!("TCP connection handler finished for {}", addr);
    Ok(())
}

#[async_trait]
impl Transport for TcpTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.lock().expect("state mutex poisoned").clone()
    }

    async fn connect(&self) -> TransportResult<()> {
        if self.remote_addr.is_some() {
            // Client mode
            self.connect_client().await
        } else {
            // Server mode
            self.start_server().await
        }
    }

    async fn disconnect(&self) -> TransportResult<()> {
        info!("Stopping TCP transport");
        *self.state.lock().expect("state mutex poisoned") = TransportState::Disconnecting;

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
        *self.state.lock().expect("state mutex poisoned") = TransportState::Disconnected;
        Ok(())
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_sent
            .fetch_add(message.size() as u64, Ordering::Relaxed);

        // Convert transport message back to JSON string for sending
        let json_str = String::from_utf8_lossy(&message.payload).to_string();

        // Send to all active connections (broadcast for server mode)
        // In client mode, there should be exactly one connection
        let connections = self.connections.lock().expect("connections mutex poisoned");
        if connections.is_empty() {
            return Err(TransportError::ConnectionFailed(
                "No active TCP connections".into(),
            ));
        }

        let mut failed_connections = Vec::new();
        for (addr, sender) in connections.iter() {
            // Use try_send with backpressure handling
            match sender.try_send(json_str.clone()) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    warn!("Connection {} channel full, applying backpressure", addr);
                    // Don't mark as failed, just apply backpressure
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    warn!("Failed to send message to TCP connection {}", addr);
                    failed_connections.push(*addr);
                }
            }
        }

        // Clean up failed connections
        drop(connections);
        if !failed_connections.is_empty() {
            let mut connections = self.connections.lock().expect("connections mutex poisoned");
            for addr in failed_connections {
                connections.remove(&addr);
            }
        }

        Ok(())
    }

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
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
                        "TCP transport channel closed".into(),
                    ))
                }
            }
        } else {
            Err(TransportError::ConnectionFailed(
                "TCP transport not connected".into(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.snapshot()
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

    /// Build the TCP transport
    #[must_use]
    pub fn build(self) -> TcpTransport {
        if let Some(remote_addr) = self.config.remote_addr {
            TcpTransport::new_client(self.config.bind_addr, remote_addr)
        } else {
            TcpTransport::new_server(self.config.bind_addr)
        }
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
            *transport.state.lock().expect("state mutex poisoned"),
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
