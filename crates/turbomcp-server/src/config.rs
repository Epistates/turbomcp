//! Server Configuration
//!
//! This module provides configuration options for MCP servers including:
//! - Protocol version negotiation
//! - Rate limiting
//! - Connection limits
//! - Capability requirements

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

// Re-export from core (single source of truth - DRY)
pub use turbomcp_core::SUPPORTED_VERSIONS as SUPPORTED_PROTOCOL_VERSIONS;

/// Default maximum connections for TCP transport.
pub const DEFAULT_MAX_CONNECTIONS: usize = 1000;

/// Default rate limit (requests per second).
pub const DEFAULT_RATE_LIMIT: u32 = 100;

/// Default rate limit window.
pub const DEFAULT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);

/// Default maximum message size (10MB).
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Protocol version configuration.
    pub protocol: ProtocolConfig,
    /// Rate limiting configuration.
    pub rate_limit: Option<RateLimitConfig>,
    /// Connection limits.
    pub connection_limits: ConnectionLimits,
    /// Required client capabilities.
    pub required_capabilities: RequiredCapabilities,
    /// Maximum message size in bytes (default: 10MB).
    pub max_message_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            protocol: ProtocolConfig::default(),
            rate_limit: None,
            connection_limits: ConnectionLimits::default(),
            required_capabilities: RequiredCapabilities::default(),
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
        }
    }
}

impl ServerConfig {
    /// Create a new server configuration with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder for server configuration.
    #[must_use]
    pub fn builder() -> ServerConfigBuilder {
        ServerConfigBuilder::default()
    }
}

/// Builder for server configuration.
#[derive(Debug, Clone, Default)]
pub struct ServerConfigBuilder {
    protocol: Option<ProtocolConfig>,
    rate_limit: Option<RateLimitConfig>,
    connection_limits: Option<ConnectionLimits>,
    required_capabilities: Option<RequiredCapabilities>,
    max_message_size: Option<usize>,
}

impl ServerConfigBuilder {
    /// Set protocol configuration.
    #[must_use]
    pub fn protocol(mut self, config: ProtocolConfig) -> Self {
        self.protocol = Some(config);
        self
    }

    /// Set rate limiting configuration.
    #[must_use]
    pub fn rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.rate_limit = Some(config);
        self
    }

    /// Set connection limits.
    #[must_use]
    pub fn connection_limits(mut self, limits: ConnectionLimits) -> Self {
        self.connection_limits = Some(limits);
        self
    }

    /// Set required client capabilities.
    #[must_use]
    pub fn required_capabilities(mut self, caps: RequiredCapabilities) -> Self {
        self.required_capabilities = Some(caps);
        self
    }

    /// Set maximum message size in bytes.
    ///
    /// Messages exceeding this size will be rejected.
    /// Default: 10MB.
    #[must_use]
    pub fn max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = Some(size);
        self
    }

    /// Build the server configuration with sensible defaults.
    ///
    /// This method always succeeds and uses defaults for any unset fields.
    /// For strict validation, use [`try_build()`](Self::try_build).
    #[must_use]
    pub fn build(self) -> ServerConfig {
        ServerConfig {
            protocol: self.protocol.unwrap_or_default(),
            rate_limit: self.rate_limit,
            connection_limits: self.connection_limits.unwrap_or_default(),
            required_capabilities: self.required_capabilities.unwrap_or_default(),
            max_message_size: self.max_message_size.unwrap_or(DEFAULT_MAX_MESSAGE_SIZE),
        }
    }

    /// Build the server configuration with validation.
    ///
    /// This method validates the configuration and returns an error if any
    /// constraints are violated. Use this for stricter configuration checking
    /// in enterprise deployments.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `max_message_size` is less than 1024 bytes (minimum viable message size)
    /// - Rate limit `max_requests` is 0
    /// - Rate limit `window` is zero
    /// - Connection limits have all values set to 0
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_server::ServerConfig;
    ///
    /// // Validated build - catches configuration errors
    /// let config = ServerConfig::builder()
    ///     .max_message_size(1024 * 1024) // 1MB
    ///     .try_build()
    ///     .expect("Invalid configuration");
    /// ```
    pub fn try_build(self) -> Result<ServerConfig, ConfigValidationError> {
        let max_message_size = self.max_message_size.unwrap_or(DEFAULT_MAX_MESSAGE_SIZE);

        // Validate message size
        if max_message_size < 1024 {
            return Err(ConfigValidationError::InvalidMessageSize {
                size: max_message_size,
                min: 1024,
            });
        }

        // Validate rate limit if provided
        if let Some(ref rate_limit) = self.rate_limit {
            if rate_limit.max_requests == 0 {
                return Err(ConfigValidationError::InvalidRateLimit {
                    reason: "max_requests cannot be 0".to_string(),
                });
            }
            if rate_limit.window.is_zero() {
                return Err(ConfigValidationError::InvalidRateLimit {
                    reason: "rate limit window cannot be zero".to_string(),
                });
            }
        }

        // Validate connection limits
        let connection_limits = self.connection_limits.unwrap_or_default();
        if connection_limits.max_tcp_connections == 0
            && connection_limits.max_websocket_connections == 0
            && connection_limits.max_http_concurrent == 0
            && connection_limits.max_unix_connections == 0
        {
            return Err(ConfigValidationError::InvalidConnectionLimits {
                reason: "at least one connection limit must be non-zero".to_string(),
            });
        }

        Ok(ServerConfig {
            protocol: self.protocol.unwrap_or_default(),
            rate_limit: self.rate_limit,
            connection_limits,
            required_capabilities: self.required_capabilities.unwrap_or_default(),
            max_message_size,
        })
    }
}

/// Errors that can occur during configuration validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigValidationError {
    /// Invalid message size configuration.
    #[error("Invalid max_message_size: {size} bytes is below minimum of {min} bytes")]
    InvalidMessageSize {
        /// The configured size.
        size: usize,
        /// The minimum allowed size.
        min: usize,
    },

    /// Invalid rate limit configuration.
    #[error("Invalid rate limit: {reason}")]
    InvalidRateLimit {
        /// Description of the validation failure.
        reason: String,
    },

    /// Invalid connection limits configuration.
    #[error("Invalid connection limits: {reason}")]
    InvalidConnectionLimits {
        /// Description of the validation failure.
        reason: String,
    },
}

/// Protocol version configuration.
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    /// Preferred protocol version.
    pub preferred_version: String,
    /// Supported protocol versions.
    pub supported_versions: Vec<String>,
    /// Allow fallback to server's preferred version if client's is unsupported.
    pub allow_fallback: bool,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            preferred_version: SUPPORTED_PROTOCOL_VERSIONS[0].to_string(),
            supported_versions: SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allow_fallback: true,
        }
    }
}

impl ProtocolConfig {
    /// Create a strict configuration that only accepts the specified version.
    #[must_use]
    pub fn strict(version: &str) -> Self {
        Self {
            preferred_version: version.to_string(),
            supported_versions: vec![version.to_string()],
            allow_fallback: false,
        }
    }

    /// Check if a protocol version is supported.
    #[must_use]
    pub fn is_supported(&self, version: &str) -> bool {
        self.supported_versions.iter().any(|v| v == version)
    }

    /// Negotiate protocol version with client.
    ///
    /// Returns the negotiated version or None if no compatible version found.
    #[must_use]
    pub fn negotiate(&self, client_version: Option<&str>) -> Option<String> {
        match client_version {
            Some(version) if self.is_supported(version) => Some(version.to_string()),
            Some(_) if self.allow_fallback => Some(self.preferred_version.clone()),
            Some(_) => None,
            None => Some(self.preferred_version.clone()),
        }
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per window.
    pub max_requests: u32,
    /// Time window for rate limiting.
    pub window: Duration,
    /// Whether to rate limit per client (by user_id or IP).
    pub per_client: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: DEFAULT_RATE_LIMIT,
            window: DEFAULT_RATE_LIMIT_WINDOW,
            per_client: true,
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration.
    #[must_use]
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            per_client: true,
        }
    }

    /// Set per-client rate limiting.
    #[must_use]
    pub fn per_client(mut self, enabled: bool) -> Self {
        self.per_client = enabled;
        self
    }
}

/// Connection limits.
#[derive(Debug, Clone)]
pub struct ConnectionLimits {
    /// Maximum concurrent TCP connections.
    pub max_tcp_connections: usize,
    /// Maximum concurrent WebSocket connections.
    pub max_websocket_connections: usize,
    /// Maximum concurrent HTTP requests.
    pub max_http_concurrent: usize,
    /// Maximum concurrent Unix socket connections.
    pub max_unix_connections: usize,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            max_tcp_connections: DEFAULT_MAX_CONNECTIONS,
            max_websocket_connections: DEFAULT_MAX_CONNECTIONS,
            max_http_concurrent: DEFAULT_MAX_CONNECTIONS,
            max_unix_connections: DEFAULT_MAX_CONNECTIONS,
        }
    }
}

impl ConnectionLimits {
    /// Create a new connection limits configuration.
    #[must_use]
    pub fn new(max_connections: usize) -> Self {
        Self {
            max_tcp_connections: max_connections,
            max_websocket_connections: max_connections,
            max_http_concurrent: max_connections,
            max_unix_connections: max_connections,
        }
    }
}

/// Required client capabilities.
///
/// Specifies which client capabilities the server requires.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequiredCapabilities {
    /// Require roots capability.
    #[serde(default)]
    pub roots: bool,
    /// Require sampling capability.
    #[serde(default)]
    pub sampling: bool,
    /// Require experimental capabilities.
    #[serde(default)]
    pub experimental: HashSet<String>,
}

impl RequiredCapabilities {
    /// Create empty required capabilities (no requirements).
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Require roots capability.
    #[must_use]
    pub fn with_roots(mut self) -> Self {
        self.roots = true;
        self
    }

    /// Require sampling capability.
    #[must_use]
    pub fn with_sampling(mut self) -> Self {
        self.sampling = true;
        self
    }

    /// Require an experimental capability.
    #[must_use]
    pub fn with_experimental(mut self, name: impl Into<String>) -> Self {
        self.experimental.insert(name.into());
        self
    }

    /// Check if all required capabilities are present in client capabilities.
    #[must_use]
    pub fn validate(&self, client_caps: &ClientCapabilities) -> CapabilityValidation {
        let mut missing = Vec::new();

        if self.roots && !client_caps.roots {
            missing.push("roots".to_string());
        }

        if self.sampling && !client_caps.sampling {
            missing.push("sampling".to_string());
        }

        for exp in &self.experimental {
            if !client_caps.experimental.contains(exp) {
                missing.push(format!("experimental/{}", exp));
            }
        }

        if missing.is_empty() {
            CapabilityValidation::Valid
        } else {
            CapabilityValidation::Missing(missing)
        }
    }
}

/// Client capabilities received during initialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Client supports roots.
    #[serde(default)]
    pub roots: bool,
    /// Client supports sampling.
    #[serde(default)]
    pub sampling: bool,
    /// Client experimental capabilities.
    #[serde(default)]
    pub experimental: HashSet<String>,
}

impl ClientCapabilities {
    /// Parse client capabilities from initialize request params.
    #[must_use]
    pub fn from_params(params: &serde_json::Value) -> Self {
        let caps = params.get("capabilities").cloned().unwrap_or_default();

        Self {
            roots: caps.get("roots").map(|v| !v.is_null()).unwrap_or(false),
            sampling: caps.get("sampling").map(|v| !v.is_null()).unwrap_or(false),
            experimental: caps
                .get("experimental")
                .and_then(|v| v.as_object())
                .map(|obj| obj.keys().cloned().collect())
                .unwrap_or_default(),
        }
    }
}

/// Result of capability validation.
#[derive(Debug, Clone)]
pub enum CapabilityValidation {
    /// All required capabilities are present.
    Valid,
    /// Some required capabilities are missing.
    Missing(Vec<String>),
}

impl CapabilityValidation {
    /// Check if validation passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// Get missing capabilities if any.
    #[must_use]
    pub fn missing(&self) -> Option<&[String]> {
        match self {
            Self::Valid => None,
            Self::Missing(caps) => Some(caps),
        }
    }
}

/// Rate limiter using token bucket algorithm.
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    /// Global bucket for non-per-client limiting.
    global_bucket: Mutex<TokenBucket>,
    /// Per-client buckets (keyed by client ID).
    client_buckets: Mutex<std::collections::HashMap<String, TokenBucket>>,
    /// Last cleanup timestamp for automatic cleanup.
    last_cleanup: Mutex<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            global_bucket: Mutex::new(TokenBucket::new(config.max_requests, config.window)),
            client_buckets: Mutex::new(std::collections::HashMap::new()),
            last_cleanup: Mutex::new(Instant::now()),
            config,
        }
    }

    /// Check if a request is allowed.
    ///
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check(&self, client_id: Option<&str>) -> bool {
        // Periodic cleanup of stale client buckets (avoid unbounded growth)
        let needs_cleanup = {
            let last = self.last_cleanup.lock();
            last.elapsed() > Duration::from_secs(60)
        };
        if needs_cleanup {
            self.cleanup(Duration::from_secs(300));
            *self.last_cleanup.lock() = Instant::now();
        }

        if self.config.per_client {
            if let Some(id) = client_id {
                let mut buckets = self.client_buckets.lock();
                let bucket = buckets.entry(id.to_string()).or_insert_with(|| {
                    TokenBucket::new(self.config.max_requests, self.config.window)
                });
                bucket.try_acquire()
            } else {
                // No client ID, use global bucket
                self.global_bucket.lock().try_acquire()
            }
        } else {
            self.global_bucket.lock().try_acquire()
        }
    }

    /// Clean up old client buckets to prevent memory growth.
    pub fn cleanup(&self, max_age: Duration) {
        let mut buckets = self.client_buckets.lock();
        let now = Instant::now();
        buckets.retain(|_, bucket| now.duration_since(bucket.last_access) < max_age);
    }

    /// Get the current number of tracked client buckets.
    #[must_use]
    pub fn client_bucket_count(&self) -> usize {
        self.client_buckets.lock().len()
    }
}

/// Token bucket for rate limiting.
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
    last_access: Instant,
}

impl TokenBucket {
    fn new(max_requests: u32, window: Duration) -> Self {
        let max_tokens = max_requests as f64;
        let refill_rate = max_tokens / window.as_secs_f64();
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
            last_access: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);

        // Only refill if meaningful time has passed (reduces syscalls on burst traffic)
        if elapsed >= Duration::from_millis(10) {
            self.tokens =
                (self.tokens + elapsed.as_secs_f64() * self.refill_rate).min(self.max_tokens);
            self.last_refill = now;
        }

        self.last_access = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Connection counter for tracking active connections.
///
/// This is designed to be wrapped in `Arc` and shared across async tasks.
/// Use `try_acquire_arc` to get a guard that can be moved into spawned tasks.
#[derive(Debug)]
pub struct ConnectionCounter {
    current: AtomicUsize,
    max: usize,
}

impl ConnectionCounter {
    /// Create a new connection counter.
    #[must_use]
    pub fn new(max: usize) -> Self {
        Self {
            current: AtomicUsize::new(0),
            max,
        }
    }

    /// Try to acquire a connection slot (for use when counter is in Arc).
    ///
    /// Returns a guard that releases the slot when dropped, or None if at capacity.
    /// The guard is `Send + 'static` and can be moved into spawned async tasks.
    pub fn try_acquire_arc(self: &Arc<Self>) -> Option<ConnectionGuard> {
        // CAS loop with bounded iterations to prevent infinite spin
        // In practice this should succeed in 1-2 iterations; 1000 indicates a bug
        for _ in 0..1000 {
            let current = self.current.load(Ordering::Relaxed);
            if current >= self.max {
                return None;
            }
            if self
                .current
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return Some(ConnectionGuard {
                    counter: Arc::clone(self),
                });
            }
            // Hint to the CPU that we're spinning (avoids pipeline stalls)
            std::hint::spin_loop();
        }
        // This should never be reached in normal operation
        tracing::error!(
            "ConnectionCounter CAS loop exceeded 1000 iterations - possible contention bug"
        );
        None
    }

    /// Get current connection count.
    #[must_use]
    pub fn current(&self) -> usize {
        self.current.load(Ordering::Relaxed)
    }

    /// Get maximum connections.
    #[must_use]
    pub fn max(&self) -> usize {
        self.max
    }

    fn release(&self) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Guard that releases a connection slot when dropped.
///
/// This guard is `Send + 'static` and can be safely moved into spawned async tasks.
#[derive(Debug)]
pub struct ConnectionGuard {
    counter: Arc<ConnectionCounter>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_negotiation_exact_match() {
        let config = ProtocolConfig::default();
        assert_eq!(
            config.negotiate(Some("2025-11-25")),
            Some("2025-11-25".to_string())
        );
    }

    #[test]
    fn test_protocol_negotiation_fallback() {
        let config = ProtocolConfig::default();
        assert_eq!(
            config.negotiate(Some("unknown-version")),
            Some("2025-11-25".to_string())
        );
    }

    #[test]
    fn test_protocol_negotiation_strict() {
        let config = ProtocolConfig::strict("2025-11-25");
        assert_eq!(config.negotiate(Some("2025-06-18")), None);
    }

    #[test]
    fn test_capability_validation() {
        let required = RequiredCapabilities::none().with_roots();
        let client = ClientCapabilities {
            roots: true,
            ..Default::default()
        };
        assert!(required.validate(&client).is_valid());

        let client_missing = ClientCapabilities::default();
        assert!(!required.validate(&client_missing).is_valid());
    }

    #[test]
    fn test_rate_limiter() {
        let config = RateLimitConfig::new(2, Duration::from_secs(1));
        let limiter = RateLimiter::new(config);

        assert!(limiter.check(None));
        assert!(limiter.check(None));
        assert!(!limiter.check(None)); // Should be rate limited
    }

    #[test]
    fn test_connection_counter() {
        let counter = Arc::new(ConnectionCounter::new(2));

        let guard1 = counter.try_acquire_arc();
        assert!(guard1.is_some());
        assert_eq!(counter.current(), 1);

        let guard2 = counter.try_acquire_arc();
        assert!(guard2.is_some());
        assert_eq!(counter.current(), 2);

        let guard3 = counter.try_acquire_arc();
        assert!(guard3.is_none()); // At capacity

        drop(guard1);
        assert_eq!(counter.current(), 1);

        let guard4 = counter.try_acquire_arc();
        assert!(guard4.is_some());
    }

    // =========================================================================
    // Builder validation tests
    // =========================================================================

    #[test]
    fn test_builder_default_succeeds() {
        // Default configuration should always succeed
        let config = ServerConfig::builder().build();
        assert_eq!(config.max_message_size, DEFAULT_MAX_MESSAGE_SIZE);
    }

    #[test]
    fn test_builder_try_build_valid() {
        let result = ServerConfig::builder()
            .max_message_size(1024 * 1024)
            .try_build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_try_build_invalid_message_size() {
        let result = ServerConfig::builder()
            .max_message_size(100) // Below minimum
            .try_build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigValidationError::InvalidMessageSize { .. }
        ));
    }

    #[test]
    fn test_builder_try_build_invalid_rate_limit() {
        let result = ServerConfig::builder()
            .rate_limit(RateLimitConfig {
                max_requests: 0, // Invalid
                window: Duration::from_secs(1),
                per_client: true,
            })
            .try_build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigValidationError::InvalidRateLimit { .. }
        ));
    }

    #[test]
    fn test_builder_try_build_zero_window() {
        let result = ServerConfig::builder()
            .rate_limit(RateLimitConfig {
                max_requests: 100,
                window: Duration::ZERO, // Invalid
                per_client: true,
            })
            .try_build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigValidationError::InvalidRateLimit { .. }
        ));
    }

    #[test]
    fn test_builder_try_build_invalid_connection_limits() {
        let result = ServerConfig::builder()
            .connection_limits(ConnectionLimits {
                max_tcp_connections: 0,
                max_websocket_connections: 0,
                max_http_concurrent: 0,
                max_unix_connections: 0,
            })
            .try_build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ConfigValidationError::InvalidConnectionLimits { .. }
        ));
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn config_builder_never_panics(
            max_msg_size in 0usize..10_000_000,
        ) {
            // Builder should never panic, just return errors for invalid inputs
            let _ = ServerConfig::builder()
                .max_message_size(max_msg_size)
                .try_build();
        }

        #[test]
        fn connection_counter_bounded(max in 1usize..10000) {
            let counter = Arc::new(ConnectionCounter::new(max));
            let mut guards = Vec::new();
            // Should never acquire more than max
            for _ in 0..max + 10 {
                if let Some(guard) = counter.try_acquire_arc() {
                    guards.push(guard);
                }
            }
            assert_eq!(guards.len(), max);
            assert_eq!(counter.current(), max);
        }
    }
}
