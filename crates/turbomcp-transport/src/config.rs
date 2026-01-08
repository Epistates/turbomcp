//! Transport configuration utilities.
//!
//! This module provides builders and presets for transport configurations.
//! The core configuration types are re-exported from `turbomcp-transport-traits`.

use std::collections::HashMap;
use std::time::Duration;

// Re-export configuration types from traits crate (via core module)
pub use crate::core::{
    LimitsConfig, TimeoutConfig, TlsConfig, TlsVersion, TransportConfig, TransportError,
    TransportResult, TransportType,
};

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
    /// use turbomcp_transport::config::{TransportConfigBuilder, LimitsConfig, TransportType};
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
    /// use turbomcp_transport::config::{TransportConfigBuilder, TimeoutConfig, TransportType};
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
    /// use turbomcp_transport::config::{TransportConfigBuilder, TlsConfig, TransportType};
    ///
    /// // Use modern TLS 1.3 (recommended)
    /// let config = TransportConfigBuilder::new(TransportType::Http)
    ///     .tls(TlsConfig::modern())
    ///     .build()
    ///     .unwrap();
    ///
    /// // Default uses TLS 1.3 (required in v3.0+)
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
        assert_eq!(timeouts.connect, Duration::from_secs(60));
        assert_eq!(timeouts.request, Some(Duration::from_secs(300))); // 5 min
        assert_eq!(timeouts.total, Some(Duration::from_secs(600))); // 10 min
        assert_eq!(timeouts.read, Some(Duration::from_secs(120)));
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
        let version = TlsVersion::default();
        assert_eq!(version, TlsVersion::Tls13);
    }

    #[test]
    fn test_tls_config_default() {
        let config = TlsConfig::default();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(config.validate_certificates);
        assert!(config.custom_ca_certs.is_none());
        assert!(config.allowed_ciphers.is_none());
    }

    #[test]
    fn test_tls_config_modern() {
        let config = TlsConfig::modern();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(config.validate_certificates);
        assert!(config.custom_ca_certs.is_none());
        assert!(config.allowed_ciphers.is_none());
        assert!(!config.is_insecure());
    }

    #[test]
    fn test_tls_config_insecure() {
        let config = TlsConfig::insecure();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert!(!config.validate_certificates);
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
        assert!(!config.is_insecure());
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
    }

    #[test]
    fn test_transport_config_includes_tls() {
        let config = TransportConfig::default();
        assert_eq!(config.tls.min_version, TlsVersion::Tls13);
        assert!(config.tls.validate_certificates);
    }
}
