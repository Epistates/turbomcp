//! Transport configuration utilities.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{TransportConfig, TransportError, TransportResult, TransportType};

/// TLS protocol version specification.
///
/// TurboMCP follows a gradual migration path to TLS 1.3:
/// - v2.2.0: TLS 1.2 default (backward compatible), TLS 1.3 recommended
/// - v2.3.0: TLS 1.3 default, TLS 1.2 deprecated with warnings (Q1 2026)
/// - v3.0.0: TLS 1.3 only, TLS 1.2 removed (Q2 2026)
///
/// # Examples
///
/// ```
/// use turbomcp_transport::config::TlsVersion;
///
/// // Use TLS 1.3 (recommended)
/// let version = TlsVersion::Tls13;
///
/// // TLS 1.2 is deprecated but still supported in v2.2.0
/// #[allow(deprecated)]
/// let legacy = TlsVersion::Tls12;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TlsVersion {
    /// TLS 1.2 protocol version.
    ///
    /// **Deprecated**: TLS 1.2 is deprecated as of v2.2.0 and will be removed in v3.0.0.
    /// Please upgrade to TLS 1.3 using `TlsConfig::modern()` or `TlsVersion::Tls13`.
    ///
    /// # Migration Timeline
    /// - v2.2.0: Deprecated (current)
    /// - v2.3.0: Removed from defaults, loud warnings
    /// - v3.0.0: Completely removed
    #[deprecated(
        since = "2.2.0",
        note = "TLS 1.2 is deprecated and will be removed in v3.0.0. Use TLS 1.3 with `TlsConfig::modern()`"
    )]
    Tls12,

    /// TLS 1.3 protocol version (recommended).
    ///
    /// TLS 1.3 provides improved security and performance over TLS 1.2:
    /// - Faster handshakes (1-RTT vs 2-RTT)
    /// - Stronger cipher suites
    /// - Better forward secrecy
    /// - Reduced attack surface
    Tls13,
}

impl Default for TlsVersion {
    /// Returns the default TLS version.
    ///
    /// In v2.2.0, this is TLS 1.2 for backward compatibility.
    /// v2.3.6: Default is now TLS 1.3 for improved security.
    fn default() -> Self {
        // v2.3.6: TLS 1.3 is the modern secure default
        Self::Tls13
    }
}

/// TLS/HTTPS configuration for secure transport connections.
///
/// This configuration applies to HTTP and WebSocket transports that use TLS.
/// It provides presets for common use cases and allows fine-grained control
/// over TLS behavior.
///
/// # Philosophy: "Secure by default, flexible by design"
///
/// - **Default**: TLS 1.2 (v2.2.0 backward compat), validates certificates
/// - **Recommended**: `TlsConfig::modern()` - TLS 1.3, validates certificates
/// - **Legacy**: `TlsConfig::legacy()` - Deprecated TLS 1.2 with warnings
/// - **Insecure**: `TlsConfig::insecure()` - For testing/mTLS mesh only
///
/// # Examples
///
/// ```
/// use turbomcp_transport::config::TlsConfig;
///
/// // Use modern TLS 1.3 (recommended)
/// let tls = TlsConfig::modern();
///
/// // Default configuration (TLS 1.2 for v2.2.0 compatibility)
/// let tls = TlsConfig::default();
///
/// // Legacy TLS 1.2 (deprecated)
/// let tls = TlsConfig::legacy();
///
/// // Disable certificate validation (testing only)
/// let tls = TlsConfig::insecure();
///
/// // Custom configuration
/// let tls = TlsConfig {
///     min_version: turbomcp_transport::config::TlsVersion::Tls13,
///     validate_certificates: true,
///     custom_ca_certs: None,
///     allowed_ciphers: None,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Minimum TLS protocol version to accept.
    ///
    /// Default: `TlsVersion::Tls12` (v2.2.0 backward compat)
    /// Recommended: `TlsVersion::Tls13`
    pub min_version: TlsVersion,

    /// Whether to validate server certificates.
    ///
    /// **Warning**: Setting this to `false` disables certificate validation,
    /// making connections vulnerable to man-in-the-middle attacks. Only use
    /// this for testing or when running in a secure mTLS mesh environment.
    ///
    /// Default: `true`
    pub validate_certificates: bool,

    /// Custom CA certificates to trust (PEM or DER format).
    ///
    /// Use this when connecting to servers with self-signed certificates
    /// or internal CAs not in the system trust store.
    ///
    /// Default: `None` (use system trust store)
    pub custom_ca_certs: Option<Vec<Vec<u8>>>,

    /// Allowed TLS cipher suites (cipher suite names).
    ///
    /// If specified, only these cipher suites will be used. If `None`,
    /// the default set of secure cipher suites will be used.
    ///
    /// Default: `None` (use library defaults)
    pub allowed_ciphers: Option<Vec<String>>,
}

impl Default for TlsConfig {
    /// Returns the default TLS configuration.
    ///
    /// In v2.2.0, this uses TLS 1.2 for backward compatibility.
    /// Starting in v2.3.0, this will use TLS 1.3.
    fn default() -> Self {
        Self {
            min_version: TlsVersion::default(),
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        }
    }
}

impl TlsConfig {
    /// Create a modern TLS 1.3 configuration (recommended).
    ///
    /// This is the recommended configuration for new deployments.
    /// It provides the best security and performance.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::TlsConfig;
    ///
    /// let tls = TlsConfig::modern();
    /// ```
    #[must_use]
    pub const fn modern() -> Self {
        Self {
            min_version: TlsVersion::Tls13,
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        }
    }

    /// Create a legacy TLS 1.2 configuration (deprecated).
    ///
    /// **Deprecated**: TLS 1.2 is deprecated and will be removed in v3.0.0.
    /// Please migrate to TLS 1.3 using `TlsConfig::modern()`.
    ///
    /// This configuration is provided for compatibility with legacy systems
    /// that do not yet support TLS 1.3.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::TlsConfig;
    ///
    /// // Only use for legacy systems that don't support TLS 1.3
    /// let tls = TlsConfig::legacy();
    /// ```
    #[must_use]
    #[deprecated(
        since = "2.2.0",
        note = "TLS 1.2 is deprecated. Use `TlsConfig::modern()` for TLS 1.3"
    )]
    pub const fn legacy() -> Self {
        #[allow(deprecated)]
        Self {
            min_version: TlsVersion::Tls12,
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        }
    }

    /// Create an insecure TLS configuration that skips certificate validation.
    ///
    /// **Warning**: This configuration is insecure and should ONLY be used:
    /// - In development/testing environments
    /// - In secure mTLS mesh environments where validation is handled elsewhere
    ///
    /// Never use this in production when connecting to untrusted servers.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::TlsConfig;
    ///
    /// // For testing only
    /// let tls = TlsConfig::insecure();
    /// ```
    #[must_use]
    pub const fn insecure() -> Self {
        Self {
            min_version: TlsVersion::Tls13,
            validate_certificates: false,
            custom_ca_certs: None,
            allowed_ciphers: None,
        }
    }

    /// Check if this configuration uses a deprecated TLS version.
    ///
    /// Returns `true` if the minimum version is TLS 1.2 or earlier.
    #[must_use]
    #[allow(deprecated)]
    pub const fn is_deprecated(&self) -> bool {
        matches!(self.min_version, TlsVersion::Tls12)
    }

    /// Check if this configuration is insecure (skips certificate validation).
    ///
    /// Returns `true` if certificate validation is disabled.
    #[must_use]
    pub const fn is_insecure(&self) -> bool {
        !self.validate_certificates
    }
}

/// Configuration for request and response size limits.
///
/// By default, TurboMCP limits response sizes to 10MB and request sizes to 1MB
/// to prevent memory exhaustion attacks. These limits can be customized or disabled
/// for environments with infrastructure-level protections (e.g., API gateways).
///
/// # Philosophy: "Secure by default, flexible by design"
///
/// - **Default:** 10MB response limit, 1MB request limit (protects most users)
/// - **Configurable:** Users can increase, decrease, or disable limits
/// - **Clear errors:** When limit hit, explain how to adjust
///
/// # Examples
///
/// ```
/// use turbomcp_transport::config::LimitsConfig;
///
/// // Use default limits (10MB response, 1MB request)
/// let limits = LimitsConfig::default();
///
/// // Increase limits for large file handling
/// let limits = LimitsConfig {
///     max_response_size: Some(50 * 1024 * 1024),  // 50MB
///     max_request_size: Some(5 * 1024 * 1024),    // 5MB
///     enforce_on_streams: true,
/// };
///
/// // Disable limits (for users behind API gateways)
/// let limits = LimitsConfig::unlimited();
///
/// // Strict limits for untrusted servers
/// let limits = LimitsConfig::strict();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsConfig {
    /// Maximum response body size in bytes.
    ///
    /// `None` = unlimited (for users behind API gateways)
    ///
    /// Default: Some(10 * 1024 * 1024) = 10MB
    pub max_response_size: Option<usize>,

    /// Maximum request body size in bytes.
    ///
    /// `None` = unlimited
    ///
    /// Default: Some(1 * 1024 * 1024) = 1MB
    pub max_request_size: Option<usize>,

    /// Whether to enforce limits on streaming responses.
    ///
    /// Default: true
    pub enforce_on_streams: bool,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_response_size: Some(10 * 1024 * 1024), // 10MB
            max_request_size: Some(1024 * 1024),       // 1MB
            enforce_on_streams: true,
        }
    }
}

impl LimitsConfig {
    /// Create a configuration with no limits.
    ///
    /// Use this when running behind infrastructure that already enforces
    /// size limits (e.g., API gateway, reverse proxy, service mesh).
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::LimitsConfig;
    ///
    /// // API Gateway (AWS, Kong) already enforces 10MB limit
    /// let limits = LimitsConfig::unlimited();
    /// ```
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_response_size: None,
            max_request_size: None,
            enforce_on_streams: false,
        }
    }

    /// Create a configuration with strict limits for untrusted servers.
    ///
    /// Use this when connecting to potentially malicious or buggy servers.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::LimitsConfig;
    ///
    /// // Connecting to untrusted public MCP server
    /// let limits = LimitsConfig::strict();
    /// ```
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            max_response_size: Some(1024 * 1024), // 1MB
            max_request_size: Some(256 * 1024),   // 256KB
            enforce_on_streams: true,
        }
    }
}

/// Configuration for request and operation timeouts.
///
/// By default, TurboMCP enforces balanced timeouts to prevent hanging requests
/// and resource exhaustion. These timeouts can be customized or disabled for
/// environments with infrastructure-level timeout management (e.g., API gateways).
///
/// # Philosophy: "Secure by default, flexible by design"
///
/// - **Default:** 30s connect, 60s request, 120s total (balanced)
/// - **Configurable:** Users can increase, decrease, or disable timeouts
/// - **Clear errors:** When timeout hit, explain how to adjust
///
/// # Examples
///
/// ```
/// use turbomcp_transport::config::TimeoutConfig;
/// use std::time::Duration;
///
/// // Use default timeouts (balanced)
/// let timeouts = TimeoutConfig::default();
///
/// // Fast operations (short timeouts)
/// let timeouts = TimeoutConfig::fast();
///
/// // Patient operations (LLM, file processing)
/// let timeouts = TimeoutConfig::patient();
///
/// // No timeouts (for users behind API gateways)
/// let timeouts = TimeoutConfig::unlimited();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Connection establishment timeout.
    ///
    /// How long to wait for TCP/TLS handshake to complete.
    ///
    /// Default: 30 seconds
    pub connect: Duration,

    /// Single request timeout.
    ///
    /// How long to wait for a single request-response cycle.
    /// `None` = no timeout (for users behind API gateways)
    ///
    /// Default: Some(60 seconds)
    pub request: Option<Duration>,

    /// Total operation timeout (including retries).
    ///
    /// Maximum time for entire operation including retries.
    /// `None` = no timeout
    ///
    /// Default: Some(120 seconds)
    pub total: Option<Duration>,

    /// Read timeout (for streaming responses).
    ///
    /// Maximum time to wait per chunk in streaming responses.
    /// `None` = no timeout
    ///
    /// Default: Some(30 seconds)
    pub read: Option<Duration>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(30),
            request: Some(Duration::from_secs(60)),
            total: Some(Duration::from_secs(120)),
            read: Some(Duration::from_secs(30)),
        }
    }
}

impl TimeoutConfig {
    /// Create a configuration with short timeouts for fast operations.
    ///
    /// Use this when you expect quick responses and want to fail fast.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::TimeoutConfig;
    ///
    /// // Quick cache lookups, simple queries
    /// let timeouts = TimeoutConfig::fast();
    /// ```
    #[must_use]
    pub const fn fast() -> Self {
        Self {
            connect: Duration::from_secs(5),
            request: Some(Duration::from_secs(10)),
            total: Some(Duration::from_secs(15)),
            read: Some(Duration::from_secs(5)),
        }
    }

    /// Create a configuration with no timeouts.
    ///
    /// Use this when running behind infrastructure that already enforces
    /// timeouts (e.g., API gateway, reverse proxy, service mesh).
    ///
    /// Note: Connect timeout is still enforced to prevent indefinite hangs.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::TimeoutConfig;
    ///
    /// // API Gateway (AWS, Kong) already enforces timeouts
    /// let timeouts = TimeoutConfig::unlimited();
    /// ```
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            connect: Duration::from_secs(30), // Keep connect timeout
            request: None,
            total: None,
            read: None,
        }
    }

    /// Create a configuration with long timeouts for slow operations.
    ///
    /// Use this for LLM sampling, large file processing, or other operations
    /// that legitimately take a long time.
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::TimeoutConfig;
    ///
    /// // LLM sampling, large file processing
    /// let timeouts = TimeoutConfig::patient();
    /// ```
    #[must_use]
    pub const fn patient() -> Self {
        Self {
            connect: Duration::from_secs(30),
            request: Some(Duration::from_secs(300)), // 5 minutes
            total: Some(Duration::from_secs(600)),   // 10 minutes
            read: Some(Duration::from_secs(60)),
        }
    }
}

/// Builder for transport configurations
#[derive(Debug, Clone)]
pub struct TransportConfigBuilder {
    transport_type: TransportType,
    connect_timeout: Duration,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    keep_alive: Option<Duration>,
    max_connections: Option<usize>,
    compression: bool,
    compression_algorithm: Option<String>,
    limits: LimitsConfig,
    timeouts: TimeoutConfig,
    tls: TlsConfig,
    custom: HashMap<String, serde_json::Value>,
}

impl TransportConfigBuilder {
    /// Create a new config builder
    #[must_use]
    pub fn new(transport_type: TransportType) -> Self {
        Self {
            transport_type,
            connect_timeout: Duration::from_secs(30),
            read_timeout: None,
            write_timeout: None,
            keep_alive: None,
            max_connections: None,
            compression: false,
            compression_algorithm: None,
            limits: LimitsConfig::default(),
            timeouts: TimeoutConfig::default(),
            tls: TlsConfig::default(),
            custom: HashMap::new(),
        }
    }

    /// Set connection timeout
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Set read timeout
    #[must_use]
    pub const fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// Set write timeout
    #[must_use]
    pub const fn write_timeout(mut self, timeout: Duration) -> Self {
        self.write_timeout = Some(timeout);
        self
    }

    /// Set keep-alive interval
    #[must_use]
    pub const fn keep_alive(mut self, interval: Duration) -> Self {
        self.keep_alive = Some(interval);
        self
    }

    /// Set maximum connections
    #[must_use]
    pub const fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = Some(max);
        self
    }

    /// Enable compression
    #[must_use]
    pub const fn enable_compression(mut self) -> Self {
        self.compression = true;
        self
    }

    /// Set compression algorithm
    pub fn compression_algorithm(mut self, algorithm: impl Into<String>) -> Self {
        self.compression_algorithm = Some(algorithm.into());
        self
    }

    /// Set size limits configuration
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::{TransportConfigBuilder, LimitsConfig};
    /// use turbomcp_transport::core::TransportType;
    ///
    /// let config = TransportConfigBuilder::new(TransportType::Http)
    ///     .limits(LimitsConfig::strict())
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub fn limits(mut self, limits: LimitsConfig) -> Self {
        self.limits = limits;
        self
    }

    /// Set timeout configuration
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::{TransportConfigBuilder, TimeoutConfig};
    /// use turbomcp_transport::core::TransportType;
    ///
    /// let config = TransportConfigBuilder::new(TransportType::Http)
    ///     .timeouts(TimeoutConfig::fast())
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub fn timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Set TLS configuration
    ///
    /// # Example
    ///
    /// ```
    /// use turbomcp_transport::config::{TransportConfigBuilder, TlsConfig};
    /// use turbomcp_transport::core::TransportType;
    ///
    /// // Use modern TLS 1.3 (recommended)
    /// let config = TransportConfigBuilder::new(TransportType::Http)
    ///     .tls(TlsConfig::modern())
    ///     .build()
    ///     .unwrap();
    ///
    /// // Or use default (TLS 1.2 for v2.2.0 compatibility)
    /// let config = TransportConfigBuilder::new(TransportType::Http)
    ///     .tls(TlsConfig::default())
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub fn tls(mut self, tls: TlsConfig) -> Self {
        self.tls = tls;
        self
    }

    /// Add custom configuration
    pub fn custom(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }

    /// Build the configuration
    pub fn build(self) -> TransportResult<TransportConfig> {
        // Validate configuration
        if self.connect_timeout < Duration::from_millis(100) {
            return Err(TransportError::ConfigurationError(
                "Connect timeout must be at least 100ms".to_string(),
            ));
        }

        if let Some(max_connections) = self.max_connections
            && max_connections == 0
        {
            return Err(TransportError::ConfigurationError(
                "Max connections must be greater than 0".to_string(),
            ));
        }

        Ok(TransportConfig {
            transport_type: self.transport_type,
            connect_timeout: self.connect_timeout,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
            keep_alive: self.keep_alive,
            max_connections: self.max_connections,
            compression: self.compression,
            compression_algorithm: self.compression_algorithm,
            limits: self.limits,
            timeouts: self.timeouts,
            tls: self.tls,
            custom: self.custom,
        })
    }
}

/// Predefined transport configurations
#[derive(Debug)]
pub struct Configs;

impl Configs {
    /// Default stdio configuration
    #[must_use]
    pub fn stdio() -> TransportConfig {
        TransportConfigBuilder::new(TransportType::Stdio)
            .build()
            .expect("Default stdio config should be valid")
    }

    /// Fast stdio configuration (shorter timeouts)
    #[must_use]
    pub fn stdio_fast() -> TransportConfig {
        TransportConfigBuilder::new(TransportType::Stdio)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("Fast stdio config should be valid")
    }

    /// Default HTTP configuration
    #[cfg(feature = "http")]
    #[must_use]
    pub fn http(port: u16) -> TransportConfig {
        TransportConfigBuilder::new(TransportType::Http)
            .custom("port", port)
            .build()
            .expect("Default HTTP config should be valid")
    }

    /// HTTP configuration with TLS
    #[cfg(all(feature = "http", feature = "tls"))]
    #[must_use]
    pub fn https(port: u16) -> TransportConfig {
        TransportConfigBuilder::new(TransportType::Http)
            .custom("port", port)
            .custom("tls", true)
            .build()
            .expect("HTTPS config should be valid")
    }

    /// Default WebSocket configuration
    #[cfg(feature = "websocket")]
    pub fn websocket(url: impl Into<String>) -> TransportConfig {
        TransportConfigBuilder::new(TransportType::WebSocket)
            .custom("url", url.into())
            .build()
            .expect("Default WebSocket config should be valid")
    }

    /// WebSocket configuration with compression
    #[cfg(all(feature = "websocket", feature = "compression"))]
    pub fn websocket_compressed(url: impl Into<String>) -> TransportConfig {
        TransportConfigBuilder::new(TransportType::WebSocket)
            .enable_compression()
            .custom("url", url.into())
            .build()
            .expect("Compressed WebSocket config should be valid")
    }

    /// Default TCP configuration
    #[cfg(feature = "tcp")]
    pub fn tcp(host: impl Into<String>, port: u16) -> TransportConfig {
        TransportConfigBuilder::new(TransportType::Tcp)
            .custom("host", host.into())
            .custom("port", port)
            .build()
            .expect("Default TCP config should be valid")
    }

    /// Default Unix socket configuration
    #[cfg(feature = "unix")]
    pub fn unix(path: impl Into<String>) -> TransportConfig {
        TransportConfigBuilder::new(TransportType::Unix)
            .custom("path", path.into())
            .build()
            .expect("Default Unix config should be valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = TransportConfigBuilder::new(TransportType::Stdio)
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(5))
            .enable_compression()
            .custom("test", "value")
            .build()
            .unwrap();

        assert_eq!(config.transport_type, TransportType::Stdio);
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.read_timeout, Some(Duration::from_secs(5)));
        assert!(config.compression);
        assert_eq!(
            config.custom.get("test"),
            Some(&serde_json::Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_config_validation() {
        // Invalid timeout
        let result = TransportConfigBuilder::new(TransportType::Stdio)
            .connect_timeout(Duration::from_millis(50))
            .build();
        assert!(result.is_err());

        // Invalid max connections
        let result = TransportConfigBuilder::new(TransportType::Stdio)
            .max_connections(0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_predefined_configs() {
        let stdio_config = Configs::stdio();
        assert_eq!(stdio_config.transport_type, TransportType::Stdio);

        let fast_stdio_config = Configs::stdio_fast();
        assert_eq!(fast_stdio_config.connect_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_timeout_config_default() {
        let timeouts = TimeoutConfig::default();
        assert_eq!(timeouts.connect, Duration::from_secs(30));
        assert_eq!(timeouts.request, Some(Duration::from_secs(60)));
        assert_eq!(timeouts.total, Some(Duration::from_secs(120)));
        assert_eq!(timeouts.read, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_timeout_config_fast() {
        let timeouts = TimeoutConfig::fast();
        assert_eq!(timeouts.connect, Duration::from_secs(5));
        assert_eq!(timeouts.request, Some(Duration::from_secs(10)));
        assert_eq!(timeouts.total, Some(Duration::from_secs(15)));
        assert_eq!(timeouts.read, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_timeout_config_unlimited() {
        let timeouts = TimeoutConfig::unlimited();
        assert_eq!(timeouts.connect, Duration::from_secs(30)); // Keep connect timeout
        assert_eq!(timeouts.request, None);
        assert_eq!(timeouts.total, None);
        assert_eq!(timeouts.read, None);
    }

    #[test]
    fn test_timeout_config_patient() {
        let timeouts = TimeoutConfig::patient();
        assert_eq!(timeouts.connect, Duration::from_secs(30));
        assert_eq!(timeouts.request, Some(Duration::from_secs(300))); // 5 min
        assert_eq!(timeouts.total, Some(Duration::from_secs(600))); // 10 min
        assert_eq!(timeouts.read, Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_config_builder_with_timeouts() {
        let config = TransportConfigBuilder::new(TransportType::Stdio)
            .timeouts(TimeoutConfig::fast())
            .build()
            .unwrap();

        assert_eq!(config.timeouts.connect, Duration::from_secs(5));
        assert_eq!(config.timeouts.request, Some(Duration::from_secs(10)));
        assert_eq!(config.timeouts.total, Some(Duration::from_secs(15)));
        assert_eq!(config.timeouts.read, Some(Duration::from_secs(5)));
    }

    // TLS Configuration Tests

    #[test]
    fn test_tls_version_default() {
        // v2.3.6: Default is TLS 1.3 for improved security
        let version = TlsVersion::default();
        assert_eq!(version, TlsVersion::Tls13);
    }

    #[test]
    fn test_tls_config_default() {
        // v2.3.6: Default is now TLS 1.3
        let config = TlsConfig::default();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(config.validate_certificates);
        assert!(config.custom_ca_certs.is_none());
        assert!(config.allowed_ciphers.is_none());
        assert!(!config.is_deprecated()); // TLS 1.3 is not deprecated
    }

    #[test]
    fn test_tls_config_modern() {
        let config = TlsConfig::modern();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(config.validate_certificates);
        assert!(config.custom_ca_certs.is_none());
        assert!(config.allowed_ciphers.is_none());
        assert!(!config.is_deprecated());
        assert!(!config.is_insecure());
    }

    #[test]
    #[allow(deprecated)]
    fn test_tls_config_legacy() {
        let config = TlsConfig::legacy();
        assert_eq!(config.min_version, TlsVersion::Tls12);
        assert!(config.validate_certificates);
        assert!(config.is_deprecated());
        assert!(!config.is_insecure());
    }

    #[test]
    fn test_tls_config_insecure() {
        let config = TlsConfig::insecure();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(!config.validate_certificates);
        assert!(!config.is_deprecated());
        assert!(config.is_insecure());
    }

    #[test]
    fn test_tls_config_custom() {
        let config = TlsConfig {
            min_version: TlsVersion::Tls13,
            validate_certificates: true,
            custom_ca_certs: Some(vec![vec![1, 2, 3]]),
            allowed_ciphers: Some(vec!["TLS_AES_256_GCM_SHA384".to_string()]),
        };

        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(config.validate_certificates);
        assert_eq!(config.custom_ca_certs.as_ref().unwrap().len(), 1);
        assert_eq!(config.allowed_ciphers.as_ref().unwrap().len(), 1);
        assert!(!config.is_deprecated());
        assert!(!config.is_insecure());
    }

    #[test]
    fn test_tls_config_is_deprecated() {
        #[allow(deprecated)]
        let tls12_config = TlsConfig {
            min_version: TlsVersion::Tls12,
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        };
        assert!(tls12_config.is_deprecated());

        let tls13_config = TlsConfig {
            min_version: TlsVersion::Tls13,
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        };
        assert!(!tls13_config.is_deprecated());
    }

    #[test]
    fn test_tls_config_is_insecure() {
        let secure = TlsConfig {
            min_version: TlsVersion::Tls13,
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        };
        assert!(!secure.is_insecure());

        let insecure = TlsConfig {
            min_version: TlsVersion::Tls13,
            validate_certificates: false,
            custom_ca_certs: None,
            allowed_ciphers: None,
        };
        assert!(insecure.is_insecure());
    }

    #[test]
    fn test_config_builder_with_tls() {
        let config = TransportConfigBuilder::new(TransportType::Http)
            .tls(TlsConfig::modern())
            .build()
            .unwrap();

        assert_eq!(config.tls.min_version, TlsVersion::Tls13);
        assert!(config.tls.validate_certificates);
        assert!(!config.tls.is_deprecated());
    }

    #[test]
    #[allow(deprecated)]
    fn test_config_builder_with_legacy_tls() {
        let config = TransportConfigBuilder::new(TransportType::Http)
            .tls(TlsConfig::legacy())
            .build()
            .unwrap();

        assert_eq!(config.tls.min_version, TlsVersion::Tls12);
        assert!(config.tls.validate_certificates);
        assert!(config.tls.is_deprecated());
    }

    #[test]
    fn test_config_builder_with_insecure_tls() {
        let config = TransportConfigBuilder::new(TransportType::Http)
            .tls(TlsConfig::insecure())
            .build()
            .unwrap();

        assert_eq!(config.tls.min_version, TlsVersion::Tls13);
        assert!(!config.tls.validate_certificates);
        assert!(config.tls.is_insecure());
    }

    #[test]
    fn test_tls_config_serialization() {
        let config = TlsConfig::modern();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: TlsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_tls_version_serialization() {
        let tls13 = TlsVersion::Tls13;
        let json = serde_json::to_string(&tls13).unwrap();
        let deserialized: TlsVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(tls13, deserialized);

        #[allow(deprecated)]
        let tls12 = TlsVersion::Tls12;
        let json = serde_json::to_string(&tls12).unwrap();
        let deserialized: TlsVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(tls12, deserialized);
    }

    #[test]
    fn test_transport_config_includes_tls() {
        // v2.3.6: Default is now TLS 1.3
        let config = TransportConfig::default();
        assert_eq!(config.tls.min_version, TlsVersion::Tls13);
        assert!(config.tls.validate_certificates);
    }
}
