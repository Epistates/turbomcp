//! MCP 2025-06-18 Compliant Streamable HTTP Transport - Types and Configuration
//!
//! This module provides configuration and session management types for HTTP transport:
//! - [`StreamableHttpConfig`] - Configuration for HTTP transport
//! - [`StreamableHttpConfigBuilder`] - Ergonomic builder for configuration
//! - [`Session`] - Session state with SSE broadcast and replay support
//! - [`StoredEvent`] - SSE event with metadata for replay
//!
//! The actual HTTP server implementation lives in `turbomcp_server::runtime::http`.
//!
//! ## Features
//!
//! - ✅ Single MCP endpoint supporting GET, POST, and DELETE
//! - ✅ SSE streaming responses from POST requests
//! - ✅ Message replay for Last-Event-ID resumability
//! - ✅ Session management with Mcp-Session-Id headers
//! - ✅ Industrial-grade security (Origin validation, rate limiting, IP binding)
//! - ✅ CORS support for browser-based clients (e.g., MCP Inspector)

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock, mpsc, oneshot};

use crate::security::{
    SecurityConfigBuilder, SecurityValidator, SessionSecurityConfig, SessionSecurityManager,
};

// TLS configuration (feature-gated)
#[cfg(feature = "tls")]
use crate::axum::config::tls::ServerTlsConfig;

// Bidirectional MCP support
use turbomcp_protocol::jsonrpc::JsonRpcResponse;

/// Type alias for pending server-initiated requests map (bidirectional MCP)
///
/// Maps request ID (String) to oneshot sender for the response.
/// Used to correlate HTTP POST responses with server-initiated requests sent via SSE.
pub type PendingRequestsMap = Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>;

/// Type alias for sessions map (useful for bidirectional dispatchers)
pub type SessionsMap = Arc<RwLock<HashMap<String, Session>>>;

/// Maximum events to buffer for replay (per session)
const MAX_REPLAY_BUFFER: usize = 1000;

/// Configuration for streamable HTTP transport
#[derive(Clone, Debug)]
pub struct StreamableHttpConfig {
    /// Bind address (default: 127.0.0.1:8080 for security)
    pub bind_addr: String,

    /// Base URL including scheme (e.g., "http://127.0.0.1:8080" or "https://...")
    ///
    /// Constructed by builder from bind_addr and TLS config.
    /// Uses "https://" when TLS is enabled, "http://" otherwise.
    /// Used for MCP endpoint discovery to ensure protocol compliance.
    pub base_url: String,

    /// Base path for MCP endpoint (default: "/mcp")
    pub endpoint_path: String,

    /// SSE keep-alive interval
    pub keep_alive: Duration,

    /// Message replay buffer size
    pub replay_buffer_size: usize,

    /// Security validator
    pub security_validator: Arc<SecurityValidator>,

    /// Session manager
    pub session_manager: Arc<SessionSecurityManager>,

    /// TLS configuration for HTTPS support (optional)
    ///
    /// When set, the server will use HTTPS instead of HTTP.
    /// Requires the `tls` feature to be enabled.
    #[cfg(feature = "tls")]
    pub tls_config: Option<ServerTlsConfig>,
}

impl Default for StreamableHttpConfig {
    fn default() -> Self {
        StreamableHttpConfigBuilder::new().build()
    }
}

/// Builder for StreamableHttpConfig with ergonomic configuration
///
/// # Examples
///
/// ```rust
/// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
/// use std::time::Duration;
///
/// // Custom rate limits for benchmarking
/// let config = StreamableHttpConfigBuilder::new()
///     .with_bind_address("127.0.0.1:3000")
///     .with_rate_limit(100_000, Duration::from_secs(60))
///     .build();
///
/// // Production configuration
/// let config = StreamableHttpConfigBuilder::new()
///     .with_bind_address("0.0.0.0:8080")
///     .with_endpoint_path("/api/mcp")
///     .with_rate_limit(1000, Duration::from_secs(60))
///     .allow_any_origin(true)
///     .require_authentication(true)
///     .build();
///
/// // Development (no rate limit)
/// let config = StreamableHttpConfigBuilder::new()
///     .without_rate_limit()
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct StreamableHttpConfigBuilder {
    bind_addr: String,
    endpoint_path: String,
    keep_alive: Duration,
    replay_buffer_size: usize,

    // Security configuration
    allow_localhost: bool,
    allow_any_origin: bool,
    require_authentication: bool,
    rate_limit: Option<(u32, Duration)>,

    // TLS configuration (feature-gated)
    #[cfg(feature = "tls")]
    tls_config: Option<ServerTlsConfig>,
}

impl Default for StreamableHttpConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamableHttpConfigBuilder {
    /// Create a new builder with sensible defaults
    pub fn new() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            endpoint_path: "/mcp".to_string(),
            keep_alive: Duration::from_secs(30),
            replay_buffer_size: MAX_REPLAY_BUFFER,
            allow_localhost: true,
            allow_any_origin: false,
            require_authentication: false,
            rate_limit: Some((100, Duration::from_secs(60))), // Default: 100 req/min
            #[cfg(feature = "tls")]
            tls_config: None,
        }
    }

    /// Set the bind address (default: "127.0.0.1:8080")
    pub fn with_bind_address(mut self, addr: impl Into<String>) -> Self {
        self.bind_addr = addr.into();
        self
    }

    /// Set the endpoint path (default: "/mcp")
    pub fn with_endpoint_path(mut self, path: impl Into<String>) -> Self {
        self.endpoint_path = path.into();
        self
    }

    /// Set the SSE keep-alive interval (default: 30 seconds)
    pub fn with_keep_alive(mut self, duration: Duration) -> Self {
        self.keep_alive = duration;
        self
    }

    /// Set the replay buffer size (default: 1000 events)
    pub fn with_replay_buffer_size(mut self, size: usize) -> Self {
        self.replay_buffer_size = size;
        self
    }

    /// Configure rate limiting (requests per time window)
    ///
    /// # Examples
    /// ```rust
    /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// use std::time::Duration;
    ///
    /// // 1000 requests per minute
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_rate_limit(1000, Duration::from_secs(60))
    ///     .build();
    ///
    /// // 100,000 requests per minute (benchmarking)
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_rate_limit(100_000, Duration::from_secs(60))
    ///     .build();
    /// ```
    pub fn with_rate_limit(mut self, requests: u32, window: Duration) -> Self {
        self.rate_limit = Some((requests, window));
        self
    }

    /// Disable rate limiting entirely
    ///
    /// # Security Warning
    /// Only disable rate limiting for benchmarks or trusted environments.
    pub fn without_rate_limit(mut self) -> Self {
        self.rate_limit = None;
        self
    }

    /// Allow localhost connections (default: true)
    pub fn allow_localhost(mut self, allow: bool) -> Self {
        self.allow_localhost = allow;
        self
    }

    /// Allow any origin for CORS (default: false)
    ///
    /// # Security Warning
    /// Only enable in development. Production should specify exact origins.
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.allow_any_origin = allow;
        self
    }

    /// Require authentication (default: false)
    pub fn require_authentication(mut self, require: bool) -> Self {
        self.require_authentication = require;
        self
    }

    /// Enable TLS/HTTPS with certificate and key files
    ///
    /// This enables HTTPS mode for the server. The server will listen for
    /// TLS connections and the base URL will use `https://` scheme.
    ///
    /// # Arguments
    ///
    /// * `cert_file` - Path to PEM-encoded certificate file
    /// * `key_file` - Path to PEM-encoded private key file
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    ///
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_bind_address("0.0.0.0:3000")
    ///     .with_tls("cert.pem", "key.pem")
    ///     .build();
    /// ```
    ///
    /// # Certificate Generation
    ///
    /// For development, generate a self-signed certificate:
    /// ```bash
    /// openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem \
    ///     -days 365 -nodes -subj "/CN=localhost"
    /// ```
    ///
    /// For production, use certificates from a trusted CA (e.g., Let's Encrypt).
    #[cfg(feature = "tls")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tls")))]
    pub fn with_tls(
        mut self,
        cert_file: impl Into<std::path::PathBuf>,
        key_file: impl Into<std::path::PathBuf>,
    ) -> Self {
        self.tls_config = Some(ServerTlsConfig::new(cert_file, key_file));
        self
    }

    /// Enable TLS/HTTPS with advanced configuration
    ///
    /// Use this method for fine-grained control over TLS settings like
    /// minimum TLS version and HTTP/2 support.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// use turbomcp_transport::axum::config::tls::{ServerTlsConfig, TlsVersion};
    ///
    /// let tls = ServerTlsConfig::new("cert.pem", "key.pem")
    ///     .with_min_version(TlsVersion::TlsV1_3)
    ///     .with_http2(true);
    ///
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_bind_address("0.0.0.0:3000")
    ///     .with_tls_config(tls)
    ///     .build();
    /// ```
    #[cfg(feature = "tls")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tls")))]
    pub fn with_tls_config(mut self, tls_config: ServerTlsConfig) -> Self {
        self.tls_config = Some(tls_config);
        self
    }

    /// Check if TLS is enabled
    #[cfg(feature = "tls")]
    pub fn is_tls_enabled(&self) -> bool {
        self.tls_config.is_some()
    }

    /// Build the configuration
    pub fn build(self) -> StreamableHttpConfig {
        let mut security_builder = SecurityConfigBuilder::new()
            .allow_localhost(self.allow_localhost)
            .allow_any_origin(self.allow_any_origin)
            .require_authentication(self.require_authentication);

        // Add rate limit if configured
        if let Some((requests, window)) = self.rate_limit {
            security_builder = security_builder.with_rate_limit(requests as usize, window);
        }

        let security_validator = Arc::new(security_builder.build());
        let session_manager =
            Arc::new(SessionSecurityManager::new(SessionSecurityConfig::default()));

        // Construct base URL with appropriate scheme based on TLS config
        #[cfg(feature = "tls")]
        let scheme = if self.tls_config.is_some() {
            "https"
        } else {
            "http"
        };
        #[cfg(not(feature = "tls"))]
        let scheme = "http";

        let base_url = format!("{}://{}", scheme, self.bind_addr);

        StreamableHttpConfig {
            bind_addr: self.bind_addr,
            base_url,
            endpoint_path: self.endpoint_path,
            keep_alive: self.keep_alive,
            replay_buffer_size: self.replay_buffer_size,
            security_validator,
            session_manager,
            #[cfg(feature = "tls")]
            tls_config: self.tls_config,
        }
    }
}

/// SSE event with metadata for replay
#[derive(Clone, Debug)]
pub struct StoredEvent {
    /// Unique event identifier for replay tracking
    pub id: String,
    /// Event type (e.g., "message", "error", "endpoint")
    pub event_type: String,
    /// Event data payload (JSON-encoded)
    pub data: String,
}

/// Session state with message replay buffer
#[derive(Debug)]
pub struct Session {
    /// Ring buffer of recent events for replay on reconnection
    pub event_buffer: VecDeque<StoredEvent>,
    /// Active SSE stream senders for broadcasting
    pub sse_senders: Vec<mpsc::UnboundedSender<StoredEvent>>,
}

impl Session {
    /// Create a new session with the specified replay buffer size
    pub fn new(buffer_size: usize) -> Self {
        Self {
            event_buffer: VecDeque::with_capacity(buffer_size),
            sse_senders: Vec::new(),
        }
    }

    /// Add event to buffer and broadcast to all connected streams
    pub fn broadcast_event(&mut self, event: StoredEvent) {
        // Add to replay buffer
        if self.event_buffer.len() >= self.event_buffer.capacity() {
            self.event_buffer.pop_front();
        }
        self.event_buffer.push_back(event.clone());

        // Broadcast to all active SSE streams
        self.sse_senders
            .retain(|sender| sender.send(event.clone()).is_ok());
    }

    /// Get events after a specific event ID for replay
    pub fn replay_from(&self, last_event_id: &str) -> Vec<StoredEvent> {
        let mut found = false;
        self.event_buffer
            .iter()
            .filter(|event| {
                if found {
                    true
                } else if event.id == last_event_id {
                    found = true;
                    false
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_defaults() {
        let config = StreamableHttpConfig::default();
        assert_eq!(config.bind_addr, "127.0.0.1:8080");
        assert_eq!(config.endpoint_path, "/mcp");
        assert_eq!(config.keep_alive, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_config_builder() {
        let config = StreamableHttpConfigBuilder::new()
            .with_bind_address("0.0.0.0:3000")
            .with_endpoint_path("/api/mcp")
            .with_keep_alive(Duration::from_secs(60))
            .allow_any_origin(true)
            .build();

        assert_eq!(config.bind_addr, "0.0.0.0:3000");
        assert_eq!(config.endpoint_path, "/api/mcp");
        assert_eq!(config.keep_alive, Duration::from_secs(60));
        assert!(config.security_validator.origin_config().allow_any);
    }

    #[tokio::test]
    async fn test_session_replay() {
        let mut session = Session::new(10);

        // Add events
        for i in 0..5 {
            session.broadcast_event(StoredEvent {
                id: format!("event-{}", i),
                event_type: "message".to_string(),
                data: format!("data-{}", i),
            });
        }

        // Replay from event-2
        let replayed = session.replay_from("event-2");
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].id, "event-3");
        assert_eq!(replayed[1].id, "event-4");
    }

    #[tokio::test]
    async fn test_session_buffer_limit() {
        let mut session = Session::new(5);

        // Add more events than buffer size
        for i in 0..10 {
            session.broadcast_event(StoredEvent {
                id: format!("event-{}", i),
                event_type: "message".to_string(),
                data: format!("data-{}", i),
            });
        }

        // Should only keep last 5
        assert_eq!(session.event_buffer.len(), 5);
        assert_eq!(session.event_buffer[0].id, "event-5");
        assert_eq!(session.event_buffer[4].id, "event-9");
    }

    #[tokio::test]
    async fn test_base_url_includes_http_scheme() {
        // REGRESSION TEST: Verify base_url includes http:// scheme
        let config = StreamableHttpConfig::default();
        assert!(
            config.base_url.starts_with("http://"),
            "Base URL must include http:// scheme. Got: {}",
            config.base_url
        );
    }
}
