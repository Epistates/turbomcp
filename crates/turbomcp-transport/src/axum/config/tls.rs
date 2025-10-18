//! TLS configuration management
//!
//! This module provides TLS (Transport Layer Security) configuration
//! for secure HTTP connections.

/// TLS version specification
#[derive(Debug, Clone)]
pub enum TlsVersion {
    /// TLS version 1.2
    TlsV1_2,
    /// TLS version 1.3
    TlsV1_3,
}

/// TLS configuration
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Certificate file path
    pub cert_file: String,
    /// Private key file path
    pub key_file: String,
    /// Minimum TLS version
    pub min_version: TlsVersion,
    /// Enable HTTP/2
    pub enable_http2: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_file: "cert.pem".to_string(),
            key_file: "key.pem".to_string(),
            min_version: TlsVersion::TlsV1_2,
            enable_http2: true,
        }
    }
}

impl TlsConfig {
    /// Create new TLS config with specific certificate and key files
    pub fn new(cert_file: String, key_file: String) -> Self {
        Self {
            cert_file,
            key_file,
            min_version: TlsVersion::TlsV1_2,
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
