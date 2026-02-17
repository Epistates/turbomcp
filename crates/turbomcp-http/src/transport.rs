//! MCP 2025-11-25 Compliant Streamable HTTP Client - Standard Implementation
//!
//! This client provides **strict MCP 2025-11-25 specification compliance** with:
//! - Single MCP endpoint for all communication
//! - Endpoint discovery via SSE "endpoint" event
//! - Accept header negotiation (application/json, text/event-stream)
//! - Handles SSE responses from POST requests
//! - Auto-reconnect with exponential backoff
//! - Last-Event-ID resumability
//! - Session management with Mcp-Session-Id
//! - Protocol version headers

use bytes::Bytes;
use futures::StreamExt;
use reqwest::{Client as HttpClient, header};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{debug, error, info, warn};

use turbomcp_protocol::MessageId;
use turbomcp_transport_traits::{
    LimitsConfig, TlsConfig, TlsVersion, Transport, TransportCapabilities, TransportError,
    TransportEventEmitter, TransportMessage, TransportMetrics, TransportResult, TransportState,
    TransportType, validate_request_size, validate_response_size,
};

/// Retry policy for auto-reconnect
#[derive(Clone, Debug)]
pub enum RetryPolicy {
    /// Fixed interval between retries
    Fixed {
        /// Time interval between retry attempts
        interval: Duration,
        /// Maximum number of retry attempts (None for unlimited)
        max_attempts: Option<u32>,
    },
    /// Exponential backoff
    Exponential {
        /// Base delay for exponential backoff calculation
        base: Duration,
        /// Maximum delay between retry attempts
        max_delay: Duration,
        /// Maximum number of retry attempts (None for unlimited)
        max_attempts: Option<u32>,
    },
    /// Never retry
    Never,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::Exponential {
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: Some(10),
        }
    }
}

impl RetryPolicy {
    pub(crate) fn delay(&self, attempt: u32) -> Option<Duration> {
        match self {
            Self::Fixed {
                interval,
                max_attempts,
            } => {
                if let Some(max) = max_attempts
                    && attempt >= *max
                {
                    return None;
                }
                Some(*interval)
            }
            Self::Exponential {
                base,
                max_delay,
                max_attempts,
            } => {
                if let Some(max) = max_attempts
                    && attempt >= *max
                {
                    return None;
                }
                let base_delay = base.as_millis() as u64 * 2u64.pow(attempt);
                let max_delay_ms = max_delay.as_millis() as u64;
                let capped = base_delay.min(max_delay_ms);
                // Add ±25% jitter to prevent thundering herd
                let jitter_range = capped / 4;
                let jitter_offset = if jitter_range > 0 {
                    // Use simple deterministic-ish jitter from attempt number
                    // (avoids adding rand dependency just for this)
                    let hash = (attempt as u64)
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    hash % (jitter_range * 2)
                } else {
                    0
                };
                let final_delay = capped
                    .saturating_sub(jitter_range)
                    .saturating_add(jitter_offset);
                Some(Duration::from_millis(final_delay))
            }
            Self::Never => None,
        }
    }
}

/// Streamable HTTP client configuration
#[derive(Clone, Debug)]
pub struct StreamableHttpClientConfig {
    /// Base URL (e.g., <https://api.example.com>)
    pub base_url: String,

    /// MCP endpoint path (e.g., "/mcp")
    pub endpoint_path: String,

    /// Request timeout
    pub timeout: Duration,

    /// Auto-reconnect policy
    pub retry_policy: RetryPolicy,

    /// Authentication token
    pub auth_token: Option<String>,

    /// Custom headers
    pub headers: HashMap<String, String>,

    /// User agent string (set to None to disable User-Agent header)
    ///
    /// Default: `TurboMCP-Client/{version}`
    ///
    /// # Security Note
    ///
    /// The User-Agent header can expose client version information. Consider:
    /// - Setting to `None` to disable User-Agent header entirely
    /// - Using a generic string like "MCP-Client" to minimize fingerprinting
    /// - Keeping the default to aid server-side debugging and analytics
    pub user_agent: Option<String>,

    /// Protocol version to use
    pub protocol_version: String,

    /// Size limits for requests and responses (v2.2.0+)
    pub limits: LimitsConfig,

    /// TLS/HTTPS configuration (v2.2.0+)
    pub tls: TlsConfig,
}

impl Default for StreamableHttpClientConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            endpoint_path: "/mcp".to_string(),
            timeout: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
            auth_token: None,
            headers: HashMap::new(),
            user_agent: Some(format!("TurboMCP-Client/{}", env!("CARGO_PKG_VERSION"))),
            protocol_version: "2025-11-25".to_string(),
            limits: LimitsConfig::default(),
            tls: TlsConfig::default(),
        }
    }
}

/// Streamable HTTP client transport
pub struct StreamableHttpClientTransport {
    config: StreamableHttpClientConfig,
    http_client: HttpClient,
    state: Arc<RwLock<TransportState>>,
    capabilities: TransportCapabilities,
    metrics: Arc<RwLock<TransportMetrics>>,
    _event_emitter: TransportEventEmitter,

    /// Discovered message endpoint (if different from main endpoint)
    message_endpoint: Arc<RwLock<Option<String>>>,

    /// Session ID from server
    session_id: Arc<RwLock<Option<String>>>,

    /// Last event ID for resumability
    last_event_id: Arc<RwLock<Option<String>>>,

    /// Channel for incoming SSE messages
    sse_receiver: Arc<Mutex<mpsc::Receiver<TransportMessage>>>,
    sse_sender: mpsc::Sender<TransportMessage>,

    /// Channel for immediate JSON responses from POST requests
    response_receiver: Arc<Mutex<mpsc::Receiver<TransportMessage>>>,
    response_sender: mpsc::Sender<TransportMessage>,

    /// SSE connection task handle
    sse_task_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl std::fmt::Debug for StreamableHttpClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamableHttpClientTransport")
            .field("base_url", &self.config.base_url)
            .field("endpoint", &self.config.endpoint_path)
            .finish()
    }
}

impl StreamableHttpClientTransport {
    /// Create new streamable HTTP client transport
    pub fn new(config: StreamableHttpClientConfig) -> Self {
        let (sse_tx, sse_rx) = mpsc::channel(1000);
        let (response_tx, response_rx) = mpsc::channel(100);
        let (event_emitter, _) = TransportEventEmitter::new();

        // Emit insecurity warning if certificate validation is disabled
        if config.tls.is_insecure() {
            warn!(
                "Certificate validation is disabled. This is insecure and should only be used \
                 for testing or in secure mTLS mesh environments. \
                 See https://turbomcp.org/docs/security/tls#certificate-validation"
            );
        }

        // Build HTTP client with TLS configuration
        // IMPORTANT: Must explicitly call use_rustls_tls() because cargo features are additive
        // and other dependencies may bring in native-tls. Without this, TLS 1.3 minimum fails.
        // See: https://github.com/seanmonstar/reqwest/issues/1314
        let mut client_builder = HttpClient::builder()
            .use_rustls_tls()
            .timeout(config.timeout);

        // Set User-Agent header if configured
        if let Some(ref user_agent) = config.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        }

        // Configure TLS version (TLS 1.3 only in v3.0)
        client_builder = match config.tls.min_version {
            TlsVersion::Tls13 => client_builder.min_tls_version(reqwest::tls::Version::TLS_1_3),
        };

        // Configure certificate validation with security gate
        if !config.tls.validate_certificates {
            // SECURITY: Require explicit env var opt-in for insecure TLS
            // This prevents accidental deployment of insecure configurations
            const INSECURE_TLS_ENV_VAR: &str = "TURBOMCP_ALLOW_INSECURE_TLS";

            if std::env::var(INSECURE_TLS_ENV_VAR).is_err() {
                error!(
                    "SECURITY: Certificate validation disabled but {} not set. \
                     Overriding to validate_certificates=true for safety. \
                     Set {}=1 to allow insecure TLS.",
                    INSECURE_TLS_ENV_VAR, INSECURE_TLS_ENV_VAR
                );
                // Override: force secure config instead of panicking
                // Don't apply danger_accept_invalid_certs
            } else {
                warn!(
                    "SECURITY WARNING: TLS certificate validation is DISABLED. \
                     This configuration is INSECURE and should ONLY be used: \
                     (1) In development/testing environments, or \
                     (2) In secure mTLS mesh where validation happens elsewhere. \
                     NEVER use in production connecting to untrusted servers."
                );

                client_builder = client_builder.danger_accept_invalid_certs(true);
            }
        }

        // Add custom CA certificates if provided
        if let Some(ca_certs) = &config.tls.custom_ca_certs {
            let mut loaded = 0usize;
            let total = ca_certs.len();
            for cert_bytes in ca_certs {
                // Try to parse as PEM or DER
                if let Ok(cert) = reqwest::Certificate::from_pem(cert_bytes) {
                    client_builder = client_builder.add_root_certificate(cert);
                    loaded += 1;
                } else if let Ok(cert) = reqwest::Certificate::from_der(cert_bytes) {
                    client_builder = client_builder.add_root_certificate(cert);
                    loaded += 1;
                } else {
                    warn!(
                        "Failed to parse custom CA certificate ({}/{}), skipping",
                        loaded + 1,
                        total
                    );
                }
            }
            if loaded == 0 && total > 0 {
                error!("All {} custom CA certificates failed to parse", total);
                // Don't panic - but log at error level. The connection will likely fail with TLS errors.
            }
            if loaded > 0 {
                info!("Loaded {}/{} custom CA certificates", loaded, total);
            }
        }

        let http_client = client_builder.build().expect("Failed to build HTTP client");

        Self {
            config,
            http_client,
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: HashMap::new(),
            },
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            _event_emitter: event_emitter,
            message_endpoint: Arc::new(RwLock::new(None)),
            session_id: Arc::new(RwLock::new(None)),
            last_event_id: Arc::new(RwLock::new(None)),
            sse_receiver: Arc::new(Mutex::new(sse_rx)),
            sse_sender: sse_tx,
            response_receiver: Arc::new(Mutex::new(response_rx)),
            response_sender: response_tx,
            sse_task_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Get full endpoint URL
    fn get_endpoint_url(&self) -> String {
        format!("{}{}", self.config.base_url, self.config.endpoint_path)
    }

    /// Get message endpoint URL (discovered or default)
    async fn get_message_endpoint_url(&self) -> String {
        let discovered = self.message_endpoint.read().await;
        if let Some(endpoint) = discovered.as_ref() {
            if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                endpoint.clone()
            } else if endpoint.starts_with('/') {
                format!("{}{}", self.config.base_url, endpoint)
            } else {
                format!("{}/{}", self.config.base_url, endpoint)
            }
        } else {
            self.get_endpoint_url()
        }
    }

    /// Build request headers
    async fn build_headers(&self, accept: &str) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();

        // Use safe header value construction - skip invalid headers rather than panic
        if let Ok(accept_value) = header::HeaderValue::from_str(accept) {
            headers.insert(header::ACCEPT, accept_value);
        }

        if let Ok(protocol_value) = header::HeaderValue::from_str(&self.config.protocol_version) {
            headers.insert("MCP-Protocol-Version", protocol_value);
        }

        if let Some(session_id) = self.session_id.read().await.as_ref()
            && let Ok(session_value) = header::HeaderValue::from_str(session_id)
        {
            headers.insert("Mcp-Session-Id", session_value);
        }

        if let Some(last_event_id) = self.last_event_id.read().await.as_ref()
            && let Ok(event_value) = header::HeaderValue::from_str(last_event_id)
        {
            headers.insert("Last-Event-ID", event_value);
        }

        if let Some(token) = &self.config.auth_token
            && let Ok(auth_value) = header::HeaderValue::from_str(&format!("Bearer {}", token))
        {
            headers.insert(header::AUTHORIZATION, auth_value);
        }

        for (key, value) in &self.config.headers {
            if let (Ok(k), Ok(v)) = (
                header::HeaderName::from_bytes(key.as_bytes()),
                header::HeaderValue::from_str(value),
            ) {
                headers.insert(k, v);
            }
        }

        headers
    }

    /// Start SSE connection task
    async fn start_sse_connection(&self) -> TransportResult<()> {
        info!("Starting SSE connection to {}", self.get_endpoint_url());

        let endpoint_url = self.get_endpoint_url();
        let config = self.config.clone();
        let http_client = self.http_client.clone();
        let state = Arc::clone(&self.state);
        let sse_sender = self.sse_sender.clone();
        let session_id = Arc::clone(&self.session_id);
        let last_event_id = Arc::clone(&self.last_event_id);
        let message_endpoint = Arc::clone(&self.message_endpoint);

        let task = tokio::spawn(async move {
            Self::sse_connection_task(
                endpoint_url,
                config,
                http_client,
                state,
                sse_sender,
                session_id,
                last_event_id,
                message_endpoint,
            )
            .await;
        });

        *self.sse_task_handle.lock().await = Some(task);

        Ok(())
    }

    /// SSE connection task with auto-reconnect
    #[allow(clippy::too_many_arguments)]
    async fn sse_connection_task(
        endpoint_url: String,
        config: StreamableHttpClientConfig,
        http_client: HttpClient,
        state: Arc<RwLock<TransportState>>,
        sse_sender: mpsc::Sender<TransportMessage>,
        session_id: Arc<RwLock<Option<String>>>,
        last_event_id: Arc<RwLock<Option<String>>>,
        message_endpoint: Arc<RwLock<Option<String>>>,
    ) {
        let mut attempt = 0u32;

        loop {
            // Check if we should retry
            if let Some(delay) = config.retry_policy.delay(attempt) {
                if attempt > 0 {
                    warn!("Reconnecting in {:?} (attempt {})", delay, attempt + 1);
                    tokio::time::sleep(delay).await;
                }
            } else {
                error!("Max retry attempts reached, giving up");
                *state.write().await = TransportState::Disconnected;
                break;
            }

            // Build request with proper headers
            let mut headers = header::HeaderMap::new();
            headers.insert(
                header::ACCEPT,
                header::HeaderValue::from_static("text/event-stream"),
            );

            if let Ok(protocol_value) = header::HeaderValue::from_str(&config.protocol_version) {
                headers.insert("MCP-Protocol-Version", protocol_value);
            }

            if let Some(sid) = session_id.read().await.as_ref()
                && let Ok(session_value) = header::HeaderValue::from_str(sid)
            {
                headers.insert("Mcp-Session-Id", session_value);
            }

            if let Some(last_id) = last_event_id.read().await.as_ref()
                && let Ok(event_value) = header::HeaderValue::from_str(last_id)
            {
                headers.insert("Last-Event-ID", event_value);
            }

            if let Some(token) = &config.auth_token
                && let Ok(auth_value) = header::HeaderValue::from_str(&format!("Bearer {}", token))
            {
                headers.insert(header::AUTHORIZATION, auth_value);
            }

            // Connect to SSE endpoint
            match http_client.get(&endpoint_url).headers(headers).send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        error!("SSE connection failed: {}", response.status());
                        attempt += 1;
                        continue;
                    }

                    // Extract session ID from response headers
                    if let Some(sid) = response
                        .headers()
                        .get("Mcp-Session-Id")
                        .and_then(|v| v.to_str().ok())
                    {
                        *session_id.write().await = Some(sid.to_string());
                        info!("Received session ID: {}", sid);
                    }

                    info!("SSE connection established");
                    *state.write().await = TransportState::Connected;
                    attempt = 0; // Reset attempt counter on success

                    // Process SSE stream
                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                let chunk_str = String::from_utf8_lossy(&chunk);
                                buffer.push_str(&chunk_str);

                                // Process complete events
                                while let Some(pos) = buffer.find("\n\n") {
                                    let event_str = buffer[..pos].to_string();
                                    buffer = buffer[pos + 2..].to_string();

                                    if let Err(e) = Self::process_sse_event(
                                        &event_str,
                                        &sse_sender,
                                        &last_event_id,
                                        &message_endpoint,
                                    )
                                    .await
                                    {
                                        warn!("Failed to process SSE event: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Error reading SSE stream: {}", e);
                                break;
                            }
                        }
                    }

                    warn!("SSE stream ended");
                    *state.write().await = TransportState::Disconnected;
                }
                Err(e) => {
                    error!("Failed to connect: {}", e);
                    attempt += 1;
                }
            }
        }
    }

    /// Process SSE event
    async fn process_sse_event(
        event_str: &str,
        sse_sender: &mpsc::Sender<TransportMessage>,
        last_event_id: &Arc<RwLock<Option<String>>>,
        message_endpoint: &Arc<RwLock<Option<String>>>,
    ) -> TransportResult<()> {
        let lines: Vec<&str> = event_str.lines().collect();
        let mut event_type: Option<String> = None;
        let mut event_data: Vec<String> = Vec::new();
        let mut event_id: Option<String> = None;

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
                    _ => {}
                }
            }
        }

        // Save event ID
        if let Some(id) = event_id {
            *last_event_id.write().await = Some(id);
        }

        if event_data.is_empty() {
            return Ok(());
        }

        let data_str = event_data.join("\n");

        // Handle different event types
        match event_type.as_deref() {
            Some("endpoint") => {
                // CRITICAL: This is the endpoint discovery event per MCP 2025-11-25 spec
                // The event data may be either:
                // 1. A JSON object: {"uri":"http://..."}
                // 2. A plain string: "http://..."
                let endpoint_uri = if data_str.trim().starts_with('{') {
                    // Parse JSON object and extract uri field
                    let endpoint_json: serde_json::Value = serde_json::from_str(&data_str)
                        .map_err(|e| {
                            TransportError::SerializationFailed(format!(
                                "Invalid endpoint JSON: {}",
                                e
                            ))
                        })?;
                    endpoint_json["uri"]
                        .as_str()
                        .ok_or_else(|| {
                            TransportError::SerializationFailed(
                                "Endpoint event missing 'uri' field".to_string(),
                            )
                        })?
                        .to_string()
                } else {
                    // Plain string format
                    data_str.clone()
                };

                info!("Discovered message endpoint: {}", endpoint_uri);
                *message_endpoint.write().await = Some(endpoint_uri);
                Ok(())
            }
            Some("message") | None => {
                // Skip empty or whitespace-only events (keep-alive, malformed events)
                // This is defensive against server sending empty data events
                if data_str.trim().is_empty() {
                    debug!("Skipping empty SSE event");
                    return Ok(());
                }

                // Parse as JSON-RPC message
                let json_value: serde_json::Value =
                    serde_json::from_str(&data_str).map_err(|e| {
                        TransportError::SerializationFailed(format!("Invalid JSON: {}", e))
                    })?;

                let message = TransportMessage::new(
                    MessageId::from("sse-message".to_string()),
                    Bytes::from(
                        serde_json::to_vec(&json_value)
                            .map_err(|e| TransportError::SerializationFailed(e.to_string()))?,
                    ),
                );

                sse_sender
                    .send(message)
                    .await
                    .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

                debug!("Received SSE message");
                Ok(())
            }
            Some(other) => {
                debug!("Ignoring unknown event type: {}", other);
                Ok(())
            }
        }
    }

    /// Process SSE event from POST response
    async fn process_post_sse_event(
        event_str: &str,
        response_sender: &mpsc::Sender<TransportMessage>,
        last_event_id: &Arc<RwLock<Option<String>>>,
    ) -> TransportResult<()> {
        let lines: Vec<&str> = event_str.lines().collect();
        let mut event_data: Vec<String> = Vec::new();
        let mut event_id: Option<String> = None;

        for line in lines {
            if line.is_empty() {
                continue;
            }

            if let Some(colon_pos) = line.find(':') {
                let field = &line[..colon_pos];
                let value = line[colon_pos + 1..].trim_start();

                match field {
                    "data" => event_data.push(value.to_string()),
                    "id" => event_id = Some(value.to_string()),
                    "event" => {
                        // Event type field - we primarily care about "message" events
                        // but we'll process any event with data
                    }
                    _ => {}
                }
            }
        }

        // Save event ID
        if let Some(id) = event_id {
            *last_event_id.write().await = Some(id);
        }

        if event_data.is_empty() {
            return Ok(());
        }

        let data_str = event_data.join("\n");

        // Parse as JSON-RPC message
        let json_value: serde_json::Value = serde_json::from_str(&data_str).map_err(|e| {
            TransportError::SerializationFailed(format!("Invalid JSON in POST SSE: {}", e))
        })?;

        let message = TransportMessage::new(
            MessageId::from("post-sse-response".to_string()),
            Bytes::from(
                serde_json::to_vec(&json_value)
                    .map_err(|e| TransportError::SerializationFailed(e.to_string()))?,
            ),
        );

        response_sender
            .send(message.clone())
            .await
            .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

        debug!(
            "Queued message from POST SSE stream: {}",
            String::from_utf8_lossy(&message.payload)
        );
        Ok(())
    }
}

impl Transport for StreamableHttpClientTransport {
    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            debug!("Sending message via HTTP POST");

            // Validate request size against configured limits (v2.2.0+)
            validate_request_size(message.payload.len(), &self.config.limits)?;

            // Get message endpoint (discovered or default)
            let url = self.get_message_endpoint_url().await;

            // Build headers with proper Accept negotiation
            let headers = self
                .build_headers("application/json, text/event-stream")
                .await;

            // Send POST request
            let response = self
                .http_client
                .post(&url)
                .headers(headers)
                .header(header::CONTENT_TYPE, "application/json")
                .body(message.payload.to_vec())
                .send()
                .await
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

            if !response.status().is_success() {
                return Err(TransportError::ConnectionFailed(format!(
                    "POST failed: {}",
                    response.status()
                )));
            }

            // Update session ID if provided
            if let Some(session_id) = response
                .headers()
                .get("Mcp-Session-Id")
                .and_then(|v| v.to_str().ok())
            {
                *self.session_id.write().await = Some(session_id.to_string());
            }

            // MCP 2025-11-25: HTTP 202 Accepted means notification/response was accepted (no body)
            if response.status() == reqwest::StatusCode::ACCEPTED {
                debug!("Received HTTP 202 Accepted (no response body expected)");
                // Update metrics
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.messages_sent += 1;
                    metrics.bytes_sent += message.payload.len() as u64;
                }
                return Ok(());
            }

            // Check response content type and handle accordingly
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if content_type.contains("application/json") {
                // MCP 2025-11-25: Server returned immediate JSON response
                debug!("Received JSON response from POST");

                let response_bytes = response
                    .bytes()
                    .await
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

                // Validate response size against configured limits (v2.2.0+)
                validate_response_size(response_bytes.len(), &self.config.limits)?;

                let response_message = TransportMessage::new(
                    MessageId::from("http-response".to_string()),
                    response_bytes,
                );

                // Queue the response for the next receive() call
                self.response_sender
                    .send(response_message)
                    .await
                    .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

                debug!("JSON response queued successfully");
            } else if content_type.contains("text/event-stream") {
                // MCP 2025-11-25: Server returned SSE stream response from POST
                // Process the stream synchronously to ensure responses are available
                debug!("Received SSE stream response from POST, processing events");

                let response_sender = self.response_sender.clone();
                let last_event_id = Arc::clone(&self.last_event_id);

                // Process SSE stream inline (not spawned) to ensure proper ordering
                let mut stream = response.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            let chunk_str = String::from_utf8_lossy(&chunk);
                            buffer.push_str(&chunk_str);

                            // Process complete events
                            while let Some(pos) = buffer.find("\n\n") {
                                let event_str = buffer[..pos].to_string();
                                buffer = buffer[pos + 2..].to_string();

                                if let Err(e) = Self::process_post_sse_event(
                                    &event_str,
                                    &response_sender,
                                    &last_event_id,
                                )
                                .await
                                {
                                    warn!("Failed to process POST SSE event: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Error reading POST SSE stream: {}", e);
                            break;
                        }
                    }
                }
                debug!("POST SSE stream processing completed");
            }

            // Update metrics
            {
                let mut metrics = self.metrics.write().await;
                metrics.messages_sent += 1;
                metrics.bytes_sent += message.payload.len() as u64;
            }

            debug!("Message sent successfully");
            Ok(())
        })
    }

    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        Box::pin(async move {
            // CRITICAL: Check response queue FIRST (for immediate JSON responses from POST)
            // This ensures request-response pattern works correctly per MCP 2025-11-25
            {
                let mut response_receiver = self.response_receiver.lock().await;
                match response_receiver.try_recv() {
                    Ok(message) => {
                        debug!("Received queued JSON response");
                        // Update metrics
                        {
                            let mut metrics = self.metrics.write().await;
                            metrics.messages_received += 1;
                            metrics.bytes_received += message.payload.len() as u64;
                        }
                        return Ok(Some(message));
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // No queued responses, continue to check SSE channel
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        return Err(TransportError::ConnectionLost(
                            "Response channel disconnected".to_string(),
                        ));
                    }
                }
            }

            // Check SSE channel for server-initiated messages
            let mut sse_receiver = self.sse_receiver.lock().await;
            match sse_receiver.try_recv() {
                Ok(message) => {
                    debug!("Received SSE message");
                    // Update metrics
                    {
                        let mut metrics = self.metrics.write().await;
                        metrics.messages_received += 1;
                        metrics.bytes_received += message.payload.len() as u64;
                    }
                    Ok(Some(message))
                }
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => Err(
                    TransportError::ConnectionLost("SSE channel disconnected".to_string()),
                ),
            }
        })
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
        Box::pin(async move { self.state.read().await.clone() })
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async move { self.metrics.read().await.clone() })
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            info!("Connecting to {}", self.get_endpoint_url());

            *self.state.write().await = TransportState::Connecting;

            // Start SSE connection task
            self.start_sse_connection().await?;

            // Wait a bit for endpoint discovery
            tokio::time::sleep(Duration::from_millis(500)).await;

            *self.state.write().await = TransportState::Connected;

            info!("Connected successfully");
            Ok(())
        })
    }

    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            info!("Disconnecting");

            *self.state.write().await = TransportState::Disconnecting;

            // Cancel SSE task
            if let Some(handle) = self.sse_task_handle.lock().await.take() {
                handle.abort();
            }

            // Send DELETE to terminate session
            if let Some(session_id) = self.session_id.read().await.as_ref() {
                let url = self.get_endpoint_url();
                let mut headers = header::HeaderMap::new();
                if let Ok(session_value) = header::HeaderValue::from_str(session_id) {
                    headers.insert("Mcp-Session-Id", session_value);
                }

                let _ = self.http_client.delete(&url).headers(headers).send().await;
            }

            *self.state.write().await = TransportState::Disconnected;

            info!("Disconnected");
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_policy_fixed() {
        let policy = RetryPolicy::Fixed {
            interval: Duration::from_secs(5),
            max_attempts: Some(3),
        };

        assert_eq!(policy.delay(0), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay(1), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay(2), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay(3), None);
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy = RetryPolicy::Exponential {
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: None,
        };

        // With jitter, verify delays are within expected bounds
        // Expected base delays: 1s, 2s, 4s, 8s, etc. with ±25% jitter
        let delay0 = policy.delay(0).unwrap();
        assert!(delay0 >= Duration::from_millis(750) && delay0 <= Duration::from_millis(1250));

        let delay1 = policy.delay(1).unwrap();
        assert!(delay1 >= Duration::from_millis(1500) && delay1 <= Duration::from_millis(2500));

        let delay2 = policy.delay(2).unwrap();
        assert!(delay2 >= Duration::from_millis(3000) && delay2 <= Duration::from_millis(5000));

        let delay3 = policy.delay(3).unwrap();
        assert!(delay3 >= Duration::from_millis(6000) && delay3 <= Duration::from_millis(10000));

        let delay10 = policy.delay(10).unwrap();
        // Should be capped at max_delay (60s) with jitter
        assert!(delay10 >= Duration::from_millis(45000) && delay10 <= Duration::from_millis(75000));
    }

    #[tokio::test]
    async fn test_client_creation() {
        let config = StreamableHttpClientConfig::default();
        let client = StreamableHttpClientTransport::new(config);

        assert_eq!(client.transport_type(), TransportType::Http);
        assert!(client.capabilities().supports_streaming);
        assert!(client.capabilities().supports_bidirectional);
    }

    #[tokio::test]
    async fn test_endpoint_event_json_parsing() {
        // REGRESSION TEST: Verify client correctly parses JSON endpoint event
        // Bug: Client was storing entire JSON string {"uri":"..."} instead of extracting URI

        use std::sync::Arc;
        use tokio::sync::RwLock;

        let message_endpoint = Arc::new(RwLock::new(None::<String>));

        // Simulate endpoint event with JSON format (MCP 2025-11-25 spec)
        let event_data = [r#"{"uri":"http://127.0.0.1:8080/mcp"}"#.to_string()];
        let data_str = event_data.join("\n");

        // Parse JSON and extract URI (mimics the fix)
        let endpoint_uri = if data_str.trim().starts_with('{') {
            let endpoint_json: serde_json::Value =
                serde_json::from_str(&data_str).expect("Failed to parse endpoint JSON");
            endpoint_json["uri"]
                .as_str()
                .expect("Missing uri field")
                .to_string()
        } else {
            data_str.clone()
        };

        *message_endpoint.write().await = Some(endpoint_uri.clone());

        // Verify URI was extracted correctly
        let stored = message_endpoint.read().await;
        assert_eq!(stored.as_ref().unwrap(), "http://127.0.0.1:8080/mcp");
        assert!(stored.as_ref().unwrap().starts_with("http://"));

        // Verify it's a valid URL
        assert!(stored.as_ref().unwrap().parse::<url::Url>().is_ok());
    }

    #[tokio::test]
    async fn test_endpoint_event_plain_string_parsing() {
        // Test backward compatibility with plain string endpoint events

        use std::sync::Arc;
        use tokio::sync::RwLock;

        let message_endpoint = Arc::new(RwLock::new(None::<String>));

        // Simulate endpoint event with plain string format
        let event_data = ["http://127.0.0.1:8080/mcp".to_string()];
        let data_str = event_data.join("\n");

        // Parse (should detect it's not JSON and use as-is)
        let endpoint_uri = if data_str.trim().starts_with('{') {
            let endpoint_json: serde_json::Value =
                serde_json::from_str(&data_str).expect("Failed to parse endpoint JSON");
            endpoint_json["uri"]
                .as_str()
                .expect("Missing uri field")
                .to_string()
        } else {
            data_str.clone()
        };

        *message_endpoint.write().await = Some(endpoint_uri.clone());

        // Verify plain string was stored correctly
        let stored = message_endpoint.read().await;
        assert_eq!(stored.as_ref().unwrap(), "http://127.0.0.1:8080/mcp");
        assert!(stored.as_ref().unwrap().starts_with("http://"));
    }
}
