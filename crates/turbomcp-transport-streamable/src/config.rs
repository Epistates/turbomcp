//! Configuration for Streamable HTTP transport.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Configuration for the Streamable HTTP transport.
#[derive(Clone, Debug)]
pub struct StreamableConfig {
    /// Maximum session duration in milliseconds.
    ///
    /// Sessions older than this are automatically terminated.
    /// Default: 24 hours (86,400,000 ms)
    pub session_timeout_ms: u64,

    /// Session idle timeout in milliseconds.
    ///
    /// Sessions with no activity for this duration are terminated.
    /// Default: 30 minutes (1,800,000 ms)
    pub idle_timeout_ms: u64,

    /// Maximum number of events to store per session for replay.
    ///
    /// Older events are discarded when this limit is reached.
    /// Default: 1000
    pub max_events_per_session: usize,

    /// SSE keepalive interval in milliseconds.
    ///
    /// Sends a comment to keep the connection alive.
    /// Default: 15 seconds (15,000 ms)
    pub keepalive_interval_ms: u64,

    /// SSE retry interval suggested to clients (in milliseconds).
    ///
    /// Clients should wait this long before reconnecting.
    /// Default: 3 seconds (3,000 ms)
    pub retry_interval_ms: u32,

    /// Whether to enable session-based routing.
    ///
    /// When enabled, requests with `Mcp-Session-Id` are routed to existing sessions.
    /// Default: true
    pub enable_sessions: bool,

    /// Allowed origins for CORS and DNS rebinding protection.
    ///
    /// If empty, all origins are allowed (not recommended for production).
    /// Default: empty (all origins)
    pub allowed_origins: Vec<String>,

    /// Whether to require origin validation.
    ///
    /// When enabled, requests without valid Origin header are rejected.
    /// Default: false
    pub require_origin: bool,

    /// Maximum request body size in bytes.
    ///
    /// Default: 1 MB (1,048,576 bytes)
    pub max_body_size: usize,

    /// Maximum concurrent SSE streams per session.
    ///
    /// Default: 1
    pub max_streams_per_session: usize,
}

impl Default for StreamableConfig {
    fn default() -> Self {
        Self {
            session_timeout_ms: 24 * 60 * 60 * 1000, // 24 hours
            idle_timeout_ms: 30 * 60 * 1000,         // 30 minutes
            max_events_per_session: 1000,
            keepalive_interval_ms: 15_000, // 15 seconds
            retry_interval_ms: 3_000,      // 3 seconds
            enable_sessions: true,
            allowed_origins: Vec::new(),
            require_origin: false,
            max_body_size: 1024 * 1024, // 1 MB
            max_streams_per_session: 1,
        }
    }
}

impl StreamableConfig {
    /// Create a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a minimal configuration for testing.
    pub fn minimal() -> Self {
        Self {
            session_timeout_ms: 60_000, // 1 minute
            idle_timeout_ms: 30_000,    // 30 seconds
            max_events_per_session: 100,
            keepalive_interval_ms: 5_000,
            retry_interval_ms: 1_000,
            enable_sessions: true,
            allowed_origins: Vec::new(),
            require_origin: false,
            max_body_size: 64 * 1024, // 64 KB
            max_streams_per_session: 1,
        }
    }

    /// Create a production configuration with sensible defaults.
    pub fn production() -> Self {
        Self {
            session_timeout_ms: 8 * 60 * 60 * 1000, // 8 hours
            idle_timeout_ms: 60 * 60 * 1000,        // 1 hour
            max_events_per_session: 5000,
            keepalive_interval_ms: 30_000, // 30 seconds
            retry_interval_ms: 5_000,      // 5 seconds
            enable_sessions: true,
            allowed_origins: Vec::new(), // Should be set explicitly
            require_origin: true,
            max_body_size: 4 * 1024 * 1024, // 4 MB
            max_streams_per_session: 2,
        }
    }

    /// Set the session timeout.
    pub fn with_session_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.session_timeout_ms = timeout_ms;
        self
    }

    /// Set the idle timeout.
    pub fn with_idle_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.idle_timeout_ms = timeout_ms;
        self
    }

    /// Set the maximum events per session.
    pub fn with_max_events(mut self, max_events: usize) -> Self {
        self.max_events_per_session = max_events;
        self
    }

    /// Set the keepalive interval.
    pub fn with_keepalive_interval_ms(mut self, interval_ms: u64) -> Self {
        self.keepalive_interval_ms = interval_ms;
        self
    }

    /// Set the retry interval.
    pub fn with_retry_interval_ms(mut self, interval_ms: u32) -> Self {
        self.retry_interval_ms = interval_ms;
        self
    }

    /// Enable or disable sessions.
    pub fn with_sessions(mut self, enable: bool) -> Self {
        self.enable_sessions = enable;
        self
    }

    /// Set allowed origins for CORS.
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }

    /// Add an allowed origin.
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        self.allowed_origins.push(origin.into());
        self
    }

    /// Require origin validation.
    pub fn with_require_origin(mut self, require: bool) -> Self {
        self.require_origin = require;
        self
    }

    /// Set maximum body size.
    pub fn with_max_body_size(mut self, size: usize) -> Self {
        self.max_body_size = size;
        self
    }

    /// Set maximum streams per session.
    pub fn with_max_streams(mut self, max_streams: usize) -> Self {
        self.max_streams_per_session = max_streams;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StreamableConfig::default();
        assert_eq!(config.session_timeout_ms, 24 * 60 * 60 * 1000);
        assert_eq!(config.idle_timeout_ms, 30 * 60 * 1000);
        assert!(config.enable_sessions);
    }

    #[test]
    fn test_minimal_config() {
        let config = StreamableConfig::minimal();
        assert_eq!(config.session_timeout_ms, 60_000);
        assert_eq!(config.max_events_per_session, 100);
    }

    #[test]
    fn test_production_config() {
        let config = StreamableConfig::production();
        assert!(config.require_origin);
        assert_eq!(config.max_streams_per_session, 2);
    }

    #[test]
    fn test_builder_pattern() {
        let config = StreamableConfig::new()
            .with_session_timeout_ms(60_000)
            .with_idle_timeout_ms(30_000)
            .with_max_events(500)
            .allow_origin("https://example.com")
            .with_require_origin(true);

        assert_eq!(config.session_timeout_ms, 60_000);
        assert_eq!(config.idle_timeout_ms, 30_000);
        assert_eq!(config.max_events_per_session, 500);
        assert_eq!(config.allowed_origins, vec!["https://example.com"]);
        assert!(config.require_origin);
    }
}
