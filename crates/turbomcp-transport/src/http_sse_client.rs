//! HTTP/SSE Client Transport for MCP
//!
//! This transport enables MCP clients to connect to HTTP-based servers using:
//! - HTTP POST for client → server requests (JSON-RPC)
//! - Server-Sent Events (SSE) for server → client push notifications
//!
//! This provides bidirectional communication over HTTP infrastructure compatible
//! with firewalls, proxies, and standard web infrastructure.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::{Client as HttpClient, header};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{debug, error, info, warn};

use turbomcp_core::MessageId;

use crate::core::{
    Transport, TransportCapabilities, TransportError, TransportEventEmitter, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};

/// HTTP/SSE client transport configuration
#[derive(Clone, Debug)]
pub struct HttpSseClientConfig {
    /// Base URL for the MCP server (e.g., "https://api.example.com/mcp")
    pub base_url: String,

    /// SSE endpoint path (relative to base_url, default: "/sse")
    pub sse_path: String,

    /// POST endpoint path (relative to base_url, default: "/rpc")
    pub post_path: String,

    /// Authentication token (if required)
    pub auth_token: Option<String>,

    /// Additional HTTP headers
    pub headers: HashMap<String, String>,

    /// Request timeout
    pub timeout: Duration,

    /// Keep-alive interval for SSE
    pub keep_alive_interval: Duration,

    /// Reconnect delay for SSE
    pub reconnect_delay: Duration,

    /// Maximum reconnection attempts (0 = infinite)
    pub max_reconnect_attempts: u32,

    /// User agent string
    pub user_agent: String,
}

impl Default for HttpSseClientConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:3000".to_string(),
            sse_path: "/sse".to_string(),
            post_path: "/rpc".to_string(),
            auth_token: None,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            keep_alive_interval: Duration::from_secs(30),
            reconnect_delay: Duration::from_secs(5),
            max_reconnect_attempts: 5,
            user_agent: format!("TurboMCP-Client/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// SSE event from server
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SseEvent {
    /// Event type
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,

    /// Event data (JSON-RPC message)
    data: serde_json::Value,

    /// Event ID for resume
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

/// HTTP/SSE client transport implementation
pub struct HttpSseClientTransport {
    /// Transport configuration
    config: HttpSseClientConfig,

    /// HTTP client for POST requests
    http_client: HttpClient,

    /// Transport state
    state: Arc<RwLock<TransportState>>,

    /// Transport capabilities
    capabilities: TransportCapabilities,

    /// Metrics collector
    metrics: Arc<RwLock<TransportMetrics>>,

    /// Event emitter
    _event_emitter: TransportEventEmitter,

    /// Channel sender for SSE messages (stored for background task)
    sse_sender: mpsc::Sender<TransportMessage>,

    /// Channel for receiving server → client messages via SSE
    sse_receiver: Arc<Mutex<mpsc::Receiver<TransportMessage>>>,

    /// SSE connection task handle
    sse_task_handle: Option<tokio::task::JoinHandle<()>>,

    /// Last SSE event ID for resume
    last_event_id: Arc<RwLock<Option<String>>>,
}

impl std::fmt::Debug for HttpSseClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpSseClientTransport")
            .field("base_url", &self.config.base_url)
            .field("state", &"<RwLock>")
            .finish()
    }
}

impl HttpSseClientTransport {
    /// Create a new HTTP/SSE client transport
    ///
    /// This creates the transport but does not start the SSE connection.
    /// Call `connect()` to establish the SSE stream.
    pub fn new(config: HttpSseClientConfig) -> Self {
        let (sse_tx, sse_rx) = mpsc::channel(1000); // Bounded channel for backpressure control
        let (event_emitter, _) = TransportEventEmitter::new();

        // Build HTTP client with timeout and user agent
        let http_client = HttpClient::builder()
            .timeout(config.timeout)
            .user_agent(&config.user_agent)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            config,
            http_client,
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: HashMap::new(),
            },
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            _event_emitter: event_emitter,
            sse_sender: sse_tx,
            sse_receiver: Arc::new(Mutex::new(sse_rx)),
            sse_task_handle: None,
            last_event_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the SSE connection (called during connect)
    async fn start_sse_connection(
        &mut self,
        sse_tx: mpsc::Sender<TransportMessage>,
    ) -> TransportResult<()> {
        info!("Starting SSE connection to {}", self.config.base_url);

        // Start SSE connection task
        let sse_url = format!("{}{}", self.config.base_url, self.config.sse_path);
        let config = self.config.clone();
        let http_client = self.http_client.clone();
        let last_event_id = Arc::clone(&self.last_event_id);
        let state = Arc::clone(&self.state);

        let task = tokio::spawn(async move {
            Self::sse_connection_task(sse_url, sse_tx, config, http_client, last_event_id, state)
                .await;
        });

        self.sse_task_handle = Some(task);

        info!("HTTP/SSE connection started successfully");
        Ok(())
    }

    /// SSE connection task (runs in background)
    async fn sse_connection_task(
        sse_url: String,
        sse_tx: mpsc::Sender<TransportMessage>,
        config: HttpSseClientConfig,
        http_client: HttpClient,
        last_event_id: Arc<RwLock<Option<String>>>,
        state: Arc<RwLock<TransportState>>,
    ) {
        let mut reconnect_attempts = 0u32;

        loop {
            // Check if we should stop reconnecting
            if config.max_reconnect_attempts > 0
                && reconnect_attempts >= config.max_reconnect_attempts
            {
                error!("Max reconnection attempts reached, giving up");
                *state.write().await = TransportState::Disconnected;
                break;
            }

            // Build SSE request
            let mut request = http_client
                .get(&sse_url)
                .header(header::ACCEPT, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache");

            // Add authentication if configured
            if let Some(ref token) = config.auth_token {
                request = request.header(header::AUTHORIZATION, format!("Bearer {}", token));
            }

            // Add Last-Event-ID for resume
            if let Some(ref event_id) = *last_event_id.read().await {
                request = request.header("Last-Event-ID", event_id);
            }

            // Add custom headers
            for (key, value) in &config.headers {
                request = request.header(key, value);
            }

            // Connect to SSE stream
            match request.send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        error!("SSE connection failed with status: {}", response.status());
                        reconnect_attempts += 1;
                        tokio::time::sleep(config.reconnect_delay).await;
                        continue;
                    }

                    info!("SSE connection established");
                    reconnect_attempts = 0;
                    *state.write().await = TransportState::Connected;

                    // Process SSE stream
                    let mut body_stream = response.bytes_stream();
                    let mut buffer = String::new();

                    while let Some(chunk_result) = body_stream.next().await {
                        let chunk_bytes = match chunk_result {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Error reading SSE stream: {}", e);
                                break;
                            }
                        };

                        let chunk_str = String::from_utf8_lossy(&chunk_bytes);
                        buffer.push_str(&chunk_str);

                        // Process complete events (separated by double newline)
                        while let Some(pos) = buffer.find("\n\n") {
                            let event_str = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            if let Err(e) =
                                Self::process_sse_event(&event_str, &sse_tx, &last_event_id).await
                            {
                                warn!("Failed to process SSE event: {}", e);
                            }
                        }
                    }

                    // Stream ended, attempt reconnect
                    warn!("SSE stream ended, reconnecting...");
                    *state.write().await = TransportState::Disconnected;
                }
                Err(e) => {
                    error!("Failed to connect to SSE endpoint: {}", e);
                    reconnect_attempts += 1;
                }
            }

            // Wait before reconnecting
            tokio::time::sleep(config.reconnect_delay).await;
        }
    }

    /// Process a single SSE event
    async fn process_sse_event(
        event_str: &str,
        sse_tx: &mpsc::Sender<TransportMessage>,
        last_event_id: &Arc<RwLock<Option<String>>>,
    ) -> TransportResult<()> {
        let lines: Vec<&str> = event_str.lines().collect();
        let mut event_type: Option<String> = None;
        let mut event_data: Vec<String> = Vec::new();
        let mut event_id: Option<String> = None;

        // Parse SSE event format
        for line in lines {
            if line.is_empty() {
                continue;
            }

            if let Some(colon_pos) = line.find(':') {
                let field = &line[..colon_pos];
                let value = line[colon_pos + 1..].trim_start();

                match field {
                    "event" => event_type = Some(value.to_string()),
                    "data" => event_data.push(value.to_string()),
                    "id" => event_id = Some(value.to_string()),
                    "retry" => {
                        // Handle retry field if needed
                        debug!("SSE retry: {}", value);
                    }
                    _ => {
                        // Ignore unknown fields
                        debug!("Unknown SSE field: {}", field);
                    }
                }
            }
        }

        // Save event ID for resume
        if let Some(id) = event_id {
            *last_event_id.write().await = Some(id);
        }

        // Process event data
        if !event_data.is_empty() {
            let data_str = event_data.join("\n");

            // Parse as JSON-RPC message
            let json_value: serde_json::Value = serde_json::from_str(&data_str).map_err(|e| {
                TransportError::SerializationFailed(format!("Invalid JSON in SSE event: {}", e))
            })?;

            // Create transport message using standard helper
            let message = TransportMessage::new(
                MessageId::from("sse-message".to_string()),
                Bytes::from(
                    serde_json::to_vec(&json_value)
                        .map_err(|e| TransportError::SerializationFailed(e.to_string()))?,
                ),
            );

            // Send to receiver channel
            sse_tx.send(message).await.map_err(|e| {
                TransportError::ConnectionLost(format!("Failed to forward SSE message: {}", e))
            })?;

            debug!("Processed SSE event: {:?}", event_type);
        }

        Ok(())
    }

    /// Get the full POST URL
    fn get_post_url(&self) -> String {
        format!("{}{}", self.config.base_url, self.config.post_path)
    }
}

#[async_trait]
impl Transport for HttpSseClientTransport {
    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        debug!("Sending HTTP POST request");

        // Build POST request
        let mut request = self
            .http_client
            .post(self.get_post_url())
            .header(header::CONTENT_TYPE, "application/json")
            .body(message.payload.to_vec());

        // Add authentication if configured
        if let Some(ref token) = self.config.auth_token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {}", token));
        }

        // Add custom headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| TransportError::ConnectionFailed(format!("HTTP POST failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(TransportError::ConnectionFailed(format!(
                "HTTP POST failed with status: {}",
                response.status()
            )));
        }

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.messages_sent += 1;
            metrics.bytes_sent += message.payload.len() as u64;
        }

        debug!("HTTP POST request sent successfully");
        Ok(())
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        // Receive from SSE channel
        let mut receiver = self.sse_receiver.lock().await;

        match receiver.try_recv() {
            Ok(message) => {
                // Update metrics
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.messages_received += 1;
                    metrics.bytes_received += message.payload.len() as u64;
                }

                Ok(Some(message))
            }
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(TransportError::ConnectionLost(
                "SSE channel disconnected".to_string(),
            )),
        }
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.read().await.clone()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        info!("Connecting to HTTP/SSE server at {}", self.config.base_url);

        // Update state to connecting
        *self.state.write().await = TransportState::Connecting;

        // Clone sender for background task
        let sse_tx = self.sse_sender.clone();

        // Start SSE connection
        self.start_sse_connection(sse_tx).await?;

        // Update state to connected
        *self.state.write().await = TransportState::Connected;

        info!("HTTP/SSE client connected successfully");
        Ok(())
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        info!("Disconnecting HTTP/SSE client transport");

        // Update state to disconnecting
        *self.state.write().await = TransportState::Disconnecting;

        // Cancel SSE task
        if let Some(handle) = self.sse_task_handle.take() {
            handle.abort();
        }

        // Update state to disconnected
        *self.state.write().await = TransportState::Disconnected;

        info!("HTTP/SSE client transport disconnected successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = HttpSseClientConfig::default();
        assert_eq!(config.base_url, "http://localhost:3000");
        assert_eq!(config.sse_path, "/sse");
        assert_eq!(config.post_path, "/rpc");
    }

    #[test]
    fn test_transport_creation() {
        let config = HttpSseClientConfig {
            base_url: "https://api.example.com/mcp".to_string(),
            ..Default::default()
        };

        let transport = HttpSseClientTransport::new(config);
        assert_eq!(transport.transport_type(), TransportType::Http);
    }
}
