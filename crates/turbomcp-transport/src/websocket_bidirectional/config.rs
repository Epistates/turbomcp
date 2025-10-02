//! Configuration types for WebSocket bidirectional transport
//!
//! This module provides configuration structures for WebSocket transport
//! including connection settings, reconnection policies, and TLS configuration.

use std::time::Duration;

/// Configuration for WebSocket bidirectional transport
#[derive(Clone, Debug)]
pub struct WebSocketBidirectionalConfig {
    /// WebSocket URL to connect to (client mode)
    pub url: Option<String>,

    /// Bind address for server mode
    pub bind_addr: Option<String>,

    /// Maximum message size (default: 16MB)
    pub max_message_size: usize,

    /// Keep-alive interval
    pub keep_alive_interval: Duration,

    /// Reconnection configuration
    pub reconnect: ReconnectConfig,

    /// Elicitation timeout
    pub elicitation_timeout: Duration,

    /// Maximum concurrent elicitations
    pub max_concurrent_elicitations: usize,

    /// Enable compression
    pub enable_compression: bool,

    /// TLS configuration
    pub tls_config: Option<TlsConfig>,
}

impl Default for WebSocketBidirectionalConfig {
    fn default() -> Self {
        Self {
            url: None,
            bind_addr: None,
            max_message_size: 16 * 1024 * 1024, // 16MB
            keep_alive_interval: Duration::from_secs(30),
            reconnect: ReconnectConfig::default(),
            elicitation_timeout: Duration::from_secs(30),
            max_concurrent_elicitations: 10,
            enable_compression: false,
            tls_config: None,
        }
    }
}

impl WebSocketBidirectionalConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create client configuration with URL
    pub fn client(url: String) -> Self {
        Self {
            url: Some(url),
            ..Self::default()
        }
    }

    /// Create server configuration with bind address
    pub fn server(bind_addr: String) -> Self {
        Self {
            bind_addr: Some(bind_addr),
            ..Self::default()
        }
    }

    /// Set maximum message size
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Set keep-alive interval
    pub fn with_keep_alive_interval(mut self, interval: Duration) -> Self {
        self.keep_alive_interval = interval;
        self
    }

    /// Set reconnection configuration
    pub fn with_reconnect_config(mut self, config: ReconnectConfig) -> Self {
        self.reconnect = config;
        self
    }

    /// Set elicitation timeout
    pub fn with_elicitation_timeout(mut self, timeout: Duration) -> Self {
        self.elicitation_timeout = timeout;
        self
    }

    /// Set maximum concurrent elicitations
    pub fn with_max_concurrent_elicitations(mut self, max: usize) -> Self {
        self.max_concurrent_elicitations = max;
        self
    }

    /// Enable compression
    pub fn with_compression(mut self, enable: bool) -> Self {
        self.enable_compression = enable;
        self
    }

    /// Set TLS configuration
    pub fn with_tls_config(mut self, tls_config: TlsConfig) -> Self {
        self.tls_config = Some(tls_config);
        self
    }
}

/// Reconnection configuration
#[derive(Clone, Debug)]
pub struct ReconnectConfig {
    /// Enable automatic reconnection
    pub enabled: bool,

    /// Initial retry delay
    pub initial_delay: Duration,

    /// Maximum retry delay
    pub max_delay: Duration,

    /// Exponential backoff factor
    pub backoff_factor: f64,

    /// Maximum number of retries
    pub max_retries: u32,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
            max_retries: 10,
        }
    }
}

impl ReconnectConfig {
    /// Create new reconnection configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable reconnection
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Create aggressive reconnection policy
    pub fn aggressive() -> Self {
        Self {
            enabled: true,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_factor: 1.5,
            max_retries: 20,
        }
    }

    /// Create conservative reconnection policy
    pub fn conservative() -> Self {
        Self {
            enabled: true,
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(60),
            backoff_factor: 2.5,
            max_retries: 5,
        }
    }

    /// Set whether reconnection is enabled
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set initial delay
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set maximum delay
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set backoff factor
    pub fn with_backoff_factor(mut self, factor: f64) -> Self {
        self.backoff_factor = factor;
        self
    }

    /// Set maximum retries
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }
}

/// TLS configuration
#[derive(Clone, Debug, Default)]
pub struct TlsConfig {
    /// Client certificate path
    pub cert_path: Option<String>,

    /// Client key path
    pub key_path: Option<String>,

    /// CA certificate path
    pub ca_path: Option<String>,

    /// Skip certificate verification (dangerous!)
    pub skip_verify: bool,
}

impl TlsConfig {
    /// Create new TLS configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create TLS configuration with certificate and key
    pub fn with_client_cert(cert_path: String, key_path: String) -> Self {
        Self {
            cert_path: Some(cert_path),
            key_path: Some(key_path),
            ..Self::default()
        }
    }

    /// Create TLS configuration with CA certificate
    pub fn with_ca_cert(ca_path: String) -> Self {
        Self {
            ca_path: Some(ca_path),
            ..Self::default()
        }
    }

    /// Create insecure TLS configuration (skip verification)
    pub fn insecure() -> Self {
        Self {
            skip_verify: true,
            ..Self::default()
        }
    }

    /// Set certificate path
    pub fn with_cert_path(mut self, path: String) -> Self {
        self.cert_path = Some(path);
        self
    }

    /// Set key path
    pub fn with_key_path(mut self, path: String) -> Self {
        self.key_path = Some(path);
        self
    }

    /// Set CA certificate path
    pub fn with_ca_path(mut self, path: String) -> Self {
        self.ca_path = Some(path);
        self
    }

    /// Set skip verification flag
    pub fn with_skip_verify(mut self, skip: bool) -> Self {
        self.skip_verify = skip;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_config_default() {
        let config = WebSocketBidirectionalConfig::default();
        assert_eq!(config.max_message_size, 16 * 1024 * 1024);
        assert_eq!(config.keep_alive_interval, Duration::from_secs(30));
        assert_eq!(config.max_concurrent_elicitations, 10);
        assert!(!config.enable_compression);
    }

    #[test]
    fn test_websocket_config_client() {
        let config = WebSocketBidirectionalConfig::client("ws://example.com".to_string());
        assert_eq!(config.url, Some("ws://example.com".to_string()));
        assert_eq!(config.bind_addr, None);
    }

    #[test]
    fn test_websocket_config_server() {
        let config = WebSocketBidirectionalConfig::server("0.0.0.0:8080".to_string());
        assert_eq!(config.bind_addr, Some("0.0.0.0:8080".to_string()));
        assert_eq!(config.url, None);
    }

    #[test]
    fn test_websocket_config_builder() {
        let config = WebSocketBidirectionalConfig::new()
            .with_max_message_size(1024)
            .with_keep_alive_interval(Duration::from_secs(60))
            .with_compression(true)
            .with_max_concurrent_elicitations(5);

        assert_eq!(config.max_message_size, 1024);
        assert_eq!(config.keep_alive_interval, Duration::from_secs(60));
        assert!(config.enable_compression);
        assert_eq!(config.max_concurrent_elicitations, 5);
    }

    #[test]
    fn test_reconnect_config_presets() {
        let aggressive = ReconnectConfig::aggressive();
        assert!(aggressive.enabled);
        assert_eq!(aggressive.initial_delay, Duration::from_millis(100));
        assert_eq!(aggressive.max_retries, 20);

        let conservative = ReconnectConfig::conservative();
        assert!(conservative.enabled);
        assert_eq!(conservative.initial_delay, Duration::from_secs(2));
        assert_eq!(conservative.max_retries, 5);

        let disabled = ReconnectConfig::disabled();
        assert!(!disabled.enabled);
    }

    #[test]
    fn test_tls_config_presets() {
        let client_cert =
            TlsConfig::with_client_cert("cert.pem".to_string(), "key.pem".to_string());
        assert_eq!(client_cert.cert_path, Some("cert.pem".to_string()));
        assert_eq!(client_cert.key_path, Some("key.pem".to_string()));

        let ca_cert = TlsConfig::with_ca_cert("ca.pem".to_string());
        assert_eq!(ca_cert.ca_path, Some("ca.pem".to_string()));

        let insecure = TlsConfig::insecure();
        assert!(insecure.skip_verify);
    }
}
