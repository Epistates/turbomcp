//! Transport configuration types.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// TLS protocol version specification.
///
/// As of TurboMCP v3.0, only TLS 1.3 is supported. TLS 1.2 support was removed
/// for improved security.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TlsVersion {
    /// TLS 1.3 protocol version (required).
    #[default]
    Tls13,
}

/// TLS/HTTPS configuration for secure transport connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Minimum TLS protocol version to accept.
    pub min_version: TlsVersion,

    /// Whether to validate server certificates.
    pub validate_certificates: bool,

    /// Custom CA certificates to trust (PEM or DER format).
    pub custom_ca_certs: Option<Vec<Vec<u8>>>,

    /// Allowed TLS cipher suites (cipher suite names).
    pub allowed_ciphers: Option<Vec<String>>,
}

impl Default for TlsConfig {
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
    #[must_use]
    pub const fn modern() -> Self {
        Self {
            min_version: TlsVersion::Tls13,
            validate_certificates: true,
            custom_ca_certs: None,
            allowed_ciphers: None,
        }
    }

    /// Create an insecure TLS configuration that skips certificate validation.
    ///
    /// **Warning**: This configuration is insecure and should ONLY be used in testing.
    #[must_use]
    pub const fn insecure() -> Self {
        Self {
            min_version: TlsVersion::Tls13,
            validate_certificates: false,
            custom_ca_certs: None,
            allowed_ciphers: None,
        }
    }

    /// Check if this configuration is insecure (skips certificate validation).
    #[must_use]
    pub const fn is_insecure(&self) -> bool {
        !self.validate_certificates
    }
}

/// Configuration for request and response size limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsConfig {
    /// Maximum response body size in bytes.
    /// `None` = unlimited
    pub max_response_size: Option<usize>,

    /// Maximum request body size in bytes.
    /// `None` = unlimited
    pub max_request_size: Option<usize>,

    /// Whether to enforce limits on streaming responses.
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
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_response_size: None,
            max_request_size: None,
            enforce_on_streams: false,
        }
    }

    /// Create a configuration with strict limits for untrusted servers.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Connection establishment timeout.
    pub connect: Duration,

    /// Single request timeout.
    /// `None` = no timeout
    pub request: Option<Duration>,

    /// Total operation timeout (including retries).
    /// `None` = no timeout
    pub total: Option<Duration>,

    /// Read timeout (for streaming responses).
    /// `None` = no timeout
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
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            connect: Duration::from_secs(30),
            request: None,
            total: None,
            read: None,
        }
    }

    /// Create a configuration with long timeouts for slow operations.
    #[must_use]
    pub const fn patient() -> Self {
        Self {
            connect: Duration::from_secs(60),
            request: Some(Duration::from_secs(300)), // 5 minutes
            total: Some(Duration::from_secs(600)),   // 10 minutes
            read: Some(Duration::from_secs(120)),    // 2 minutes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_default() {
        let config = TlsConfig::default();
        assert!(config.validate_certificates);
        assert!(!config.is_insecure());
    }

    #[test]
    fn test_limits_config_default() {
        let config = LimitsConfig::default();
        assert_eq!(config.max_response_size, Some(10 * 1024 * 1024));
        assert_eq!(config.max_request_size, Some(1024 * 1024));
    }

    #[test]
    fn test_timeout_config_default() {
        let config = TimeoutConfig::default();
        assert_eq!(config.connect, Duration::from_secs(30));
        assert_eq!(config.request, Some(Duration::from_secs(60)));
    }
}
