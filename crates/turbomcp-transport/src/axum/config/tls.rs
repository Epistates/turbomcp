//! TLS configuration management for HTTP servers
//!
//! This module provides TLS (Transport Layer Security) configuration
//! for secure HTTPS connections. It supports certificate loading from
//! PEM files and integrates with rustls for modern TLS support.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_transport::axum::config::tls::{ServerTlsConfig, TlsVersion};
//!
//! // Simple configuration
//! let tls = ServerTlsConfig::new("cert.pem", "key.pem");
//!
//! // Advanced configuration
//! let tls = ServerTlsConfig::new("cert.pem", "key.pem")
//!     .with_min_version(TlsVersion::TlsV1_3)
//!     .with_http2(true);
//! ```

use std::path::PathBuf;

/// TLS version specification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    /// TLS version 1.2 (legacy, but still widely supported)
    TlsV1_2,
    /// TLS version 1.3 (recommended for modern deployments)
    TlsV1_3,
}

/// TLS configuration error
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    /// Failed to read certificate file
    #[error("Failed to read certificate file '{path}': {source}")]
    CertificateReadError {
        /// Path to the certificate file
        path: PathBuf,
        /// IO error that occurred
        #[source]
        source: std::io::Error,
    },

    /// Failed to read private key file
    #[error("Failed to read private key file '{path}': {source}")]
    KeyReadError {
        /// Path to the key file
        path: PathBuf,
        /// IO error that occurred
        #[source]
        source: std::io::Error,
    },

    /// No valid certificates found in PEM file
    #[error("No valid certificates found in '{path}'")]
    NoCertificates {
        /// Path to the certificate file
        path: PathBuf,
    },

    /// No valid private key found in PEM file
    #[error("No valid private key found in '{path}'")]
    NoPrivateKey {
        /// Path to the key file
        path: PathBuf,
    },

    /// Invalid certificate or key format
    #[error("Invalid certificate or key: {0}")]
    InvalidCertificate(String),

    /// Rustls configuration error
    #[cfg(feature = "tls")]
    #[error("TLS configuration error: {0}")]
    RustlsError(#[from] rustls::Error),
}

/// Server-side TLS configuration for HTTPS connections
///
/// This struct configures TLS for HTTP servers, including certificate
/// and private key paths, TLS version requirements, and HTTP/2 support.
///
/// # Security
///
/// - Defaults to TLS 1.3 for maximum security
/// - Uses safe cipher suites via rustls
/// - Supports HTTP/2 via ALPN negotiation
#[derive(Debug, Clone)]
pub struct ServerTlsConfig {
    /// Path to the PEM-encoded certificate file
    pub cert_file: PathBuf,
    /// Path to the PEM-encoded private key file
    pub key_file: PathBuf,
    /// Minimum TLS version (defaults to TLS 1.3)
    pub min_version: TlsVersion,
    /// Enable HTTP/2 via ALPN (defaults to true)
    pub enable_http2: bool,
}

impl Default for ServerTlsConfig {
    fn default() -> Self {
        Self {
            cert_file: PathBuf::from("cert.pem"),
            key_file: PathBuf::from("key.pem"),
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        }
    }
}

impl ServerTlsConfig {
    /// Create new TLS config with specific certificate and key files
    ///
    /// # Arguments
    ///
    /// * `cert_file` - Path to PEM-encoded certificate file
    /// * `key_file` - Path to PEM-encoded private key file
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tls = ServerTlsConfig::new("certs/server.pem", "certs/server.key");
    /// ```
    pub fn new(cert_file: impl Into<PathBuf>, key_file: impl Into<PathBuf>) -> Self {
        Self {
            cert_file: cert_file.into(),
            key_file: key_file.into(),
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        }
    }

    /// Set minimum TLS version
    ///
    /// # Arguments
    ///
    /// * `version` - Minimum TLS version to accept
    ///
    /// # Security Note
    ///
    /// TLS 1.3 is recommended. Only use TLS 1.2 if required for
    /// compatibility with older clients.
    pub fn with_min_version(mut self, version: TlsVersion) -> Self {
        self.min_version = version;
        self
    }

    /// Enable or disable HTTP/2 via ALPN
    ///
    /// HTTP/2 provides better performance through multiplexing.
    /// Enabled by default.
    pub fn with_http2(mut self, enable: bool) -> Self {
        self.enable_http2 = enable;
        self
    }

    /// Load certificates and create a rustls ServerConfig
    ///
    /// This method reads the certificate and key files, validates them,
    /// and creates a rustls configuration ready for use with TlsAcceptor.
    ///
    /// # Errors
    ///
    /// Returns `TlsError` if:
    /// - Certificate or key files cannot be read
    /// - No valid certificates found in the PEM file
    /// - No valid private key found in the PEM file
    /// - Certificate/key pair is invalid
    #[cfg(feature = "tls")]
    pub fn load_rustls_config(&self) -> Result<std::sync::Arc<rustls::ServerConfig>, TlsError> {
        use rustls::pki_types::PrivateKeyDer;
        use rustls_pemfile::{certs, private_key};
        use std::fs::File;
        use std::io::BufReader;

        // Read certificate file
        let cert_file =
            File::open(&self.cert_file).map_err(|e| TlsError::CertificateReadError {
                path: self.cert_file.clone(),
                source: e,
            })?;

        // Read private key file
        let key_file = File::open(&self.key_file).map_err(|e| TlsError::KeyReadError {
            path: self.key_file.clone(),
            source: e,
        })?;

        // Parse certificates
        let certs: Vec<_> = certs(&mut BufReader::new(cert_file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TlsError::InvalidCertificate(e.to_string()))?;

        if certs.is_empty() {
            return Err(TlsError::NoCertificates {
                path: self.cert_file.clone(),
            });
        }

        // Parse private key
        let key: PrivateKeyDer<'_> = private_key(&mut BufReader::new(key_file))
            .map_err(|e| TlsError::InvalidCertificate(e.to_string()))?
            .ok_or_else(|| TlsError::NoPrivateKey {
                path: self.key_file.clone(),
            })?;

        // Install default crypto provider if not already installed
        // This is required for rustls 0.23+ which doesn't auto-detect providers
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // Build rustls config
        let mut config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        // Configure ALPN protocols
        // Note: Currently our manual TLS serving only supports HTTP/1.1
        // HTTP/2 support will be added in a future release
        // For now, we only advertise HTTP/1.1 to avoid protocol negotiation issues
        config.alpn_protocols = vec![b"http/1.1".to_vec()];

        Ok(std::sync::Arc::new(config))
    }
}

// Re-export as TlsConfig for backward compatibility
#[doc(hidden)]
#[deprecated(since = "2.4.0", note = "Use ServerTlsConfig instead")]
pub type TlsConfig = ServerTlsConfig;

#[cfg(all(test, feature = "tls"))]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Self-signed test certificate (DO NOT USE IN PRODUCTION)
    const TEST_CERT: &str = r#"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKHBfpegPjMCMA0GCSqGSIb3DQEBCwUAMBExDzANBgNVBAMMBnVu
dXNlZDAeFw0yNDAxMDEwMDAwMDBaFw0yNTAxMDEwMDAwMDBaMBExDzANBgNVBAMM
BnVudXNlZDBcMA0GCSqGSIb3DQEBAQUAA0sAMEgCQQC6GoJCWTYzQAn/AxN0JDPK
T4b8TLChN36HYvpNqP7Y3Y6uPr7AqKfNGwOjPOjE7IqN7CWHzL0b0lP7TzZ7Yr7P
AgMBAAGjUzBRMB0GA1UdDgQWBBQwZTDLLmJYWvP7L2qmPKPnPJNvgTAfBgNVHSME
GDAWgBQwZTDLLmJYWvP7L2qmPKPnPJNvgTAPBgNVHRMBAf8EBTADAQH/MA0GCSqG
SIb3DQEBCwUAA0EAOvPQi1jHGnKGrwanPJrkl5JUbzCM7IaF7JJN7IJQNLK0VnOP
E4Y3BPBH2mlCFEzqcQ3x0P8yrPKIqGIFvAzEQQ==
-----END CERTIFICATE-----"#;

    const TEST_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIBVQIBADANBgkqhkiG9w0BAQEFAASCAT8wggE7AgEAAkEAuhqCQlk2M0AJ/wMT
dCQzyk+G/EywoTd+h2L6Taj+2N2Orj6+wKinzRsDozzoxOyKjewlh8y9G9JT+082
e2K+zwIDAQABAkA1v8j6x3a7NHmZvP7h8+y7dE4+4C6hN7EWj5E8DTZ2CtGxPJZN
qm6qPJNZYvPKKGF7R7qwI3X0L6m7k4m5N1GBAiEA5vT3VPxEF6Wk7fP0g6E7m7A8
7e9EF7FPFxJZsZ7L7gECIQDO0mV7j7OJj3HnX7Y7c3F7d2J7F7pJ7B7M7n7C7L7v
bwIgBmKO7X7pL7d7D7X7d7k7z7d7B7J7H7F7G7c7N7f7EAECIQCFqE7J7k7X7j7R
7P7J7W7h7f7n7x7Y7L7D7p7P7c7TpwIhAIZ7T7G7H7p7l7r7m7K7n7q7B7X7z7s7
N7f7c7k7x7IA
-----END PRIVATE KEY-----"#;

    #[test]
    fn test_default_config() {
        let config = ServerTlsConfig::default();
        assert_eq!(config.cert_file, PathBuf::from("cert.pem"));
        assert_eq!(config.key_file, PathBuf::from("key.pem"));
        assert_eq!(config.min_version, TlsVersion::TlsV1_3);
        assert!(config.enable_http2);
    }

    #[test]
    fn test_builder_pattern() {
        let config = ServerTlsConfig::new("server.crt", "server.key")
            .with_min_version(TlsVersion::TlsV1_2)
            .with_http2(false);

        assert_eq!(config.cert_file, PathBuf::from("server.crt"));
        assert_eq!(config.key_file, PathBuf::from("server.key"));
        assert_eq!(config.min_version, TlsVersion::TlsV1_2);
        assert!(!config.enable_http2);
    }

    #[test]
    fn test_missing_cert_file() {
        let config = ServerTlsConfig::new("/nonexistent/cert.pem", "/nonexistent/key.pem");
        let result = config.load_rustls_config();
        assert!(matches!(result, Err(TlsError::CertificateReadError { .. })));
    }

    #[test]
    fn test_missing_key_file() {
        // Create a temporary cert file
        let mut cert_file = NamedTempFile::new().unwrap();
        cert_file.write_all(TEST_CERT.as_bytes()).unwrap();

        let config = ServerTlsConfig::new(cert_file.path(), "/nonexistent/key.pem");
        let result = config.load_rustls_config();
        assert!(matches!(result, Err(TlsError::KeyReadError { .. })));
    }
}
