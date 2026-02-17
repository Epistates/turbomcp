//! TLS configuration management
//!
//! This module provides TLS (Transport Layer Security) configuration
//! for secure HTTP connections.
//!
//! # Security Note
//!
//! TLS 1.3 is required for all configurations in v3.
//! TLS 1.3 provides strong security guarantees including:
//! - Perfect forward secrecy by default
//! - Improved handshake performance (1-RTT)
//! - Removal of legacy cryptographic algorithms

/// TLS version specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsVersion {
    /// TLS version 1.3 (required)
    #[default]
    TlsV1_3,
}

/// TLS configuration
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Certificate file path
    pub cert_file: String,
    /// Private key file path
    pub key_file: String,
    /// Minimum TLS version (defaults to TLS 1.3)
    pub min_version: TlsVersion,
    /// Enable HTTP/2
    pub enable_http2: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_file: "cert.pem".to_string(),
            key_file: "key.pem".to_string(),
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        }
    }
}

impl TlsConfig {
    /// Create new TLS config with specific certificate and key files
    ///
    /// Uses TLS 1.3 as the minimum version by default.
    pub fn new(cert_file: String, key_file: String) -> Self {
        Self {
            cert_file,
            key_file,
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        }
    }

    /// Set minimum TLS version
    pub fn with_min_version(mut self, version: TlsVersion) -> Self {
        self.min_version = version;
        self
    }

    /// Enable or disable HTTP/2
    pub fn with_http2(mut self, enable: bool) -> Self {
        self.enable_http2 = enable;
        self
    }
}
