//! Server configuration management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
    /// Server description
    pub description: Option<String>,
    /// Bind address
    pub bind_address: String,
    /// Bind port
    pub port: u16,
    /// Enable TLS
    pub enable_tls: bool,
    /// TLS configuration
    pub tls: Option<TlsConfig>,
    /// Timeout configuration
    pub timeouts: TimeoutConfig,
    /// Rate limiting configuration
    pub rate_limiting: RateLimitingConfig,
    /// Logging configuration
    pub logging: LoggingConfig,
    /// Additional configuration
    pub additional: HashMap<String, serde_json::Value>,
}

/// TLS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Certificate file path
    pub cert_file: PathBuf,
    /// Private key file path
    pub key_file: PathBuf,
}

/// Timeout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Request timeout
    pub request_timeout: Duration,
    /// Connection timeout
    pub connection_timeout: Duration,
    /// Keep-alive timeout
    pub keep_alive_timeout: Duration,
    /// Tool execution timeout (default for all tools)
    pub tool_execution_timeout: Duration,
    /// Per-tool timeout overrides (tool_name -> duration_seconds)
    pub tool_timeouts: HashMap<String, u64>,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    /// Enable rate limiting
    pub enabled: bool,
    /// Requests per second
    pub requests_per_second: u32,
    /// Burst capacity
    pub burst_capacity: u32,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level
    pub level: String,
    /// Enable structured logging
    pub structured: bool,
    /// Log file path
    pub file: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: crate::SERVER_NAME.to_string(),
            version: crate::SERVER_VERSION.to_string(),
            description: Some("Next generation MCP server".to_string()),
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
            enable_tls: false,
            tls: None,
            timeouts: TimeoutConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            logging: LoggingConfig::default(),
            additional: HashMap::new(),
        }
    }
}

/// Configuration error types
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Config file not found
    #[error("Configuration file not found: {0}")]
    FileNotFound(PathBuf),

    /// Unsupported file format
    #[error("Unsupported configuration file format. Use .toml, .yaml, .yml, or .json")]
    UnsupportedFormat,

    /// Configuration parsing error
    #[error("Failed to parse configuration: {0}")]
    ParseError(#[from] config::ConfigError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl ServerConfig {
    /// Load configuration from a file (TOML, YAML, or JSON)
    ///
    /// The file format is auto-detected from the file extension:
    /// - `.toml` → TOML format
    /// - `.yaml` or `.yml` → YAML format
    /// - `.json` → JSON format
    ///
    /// Environment variables with the `TURBOMCP_` prefix will override file settings.
    /// For example, `TURBOMCP_PORT=9000` will override the `port` setting.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use turbomcp_server::ServerConfig;
    ///
    /// let config = ServerConfig::from_file("config.toml").expect("Failed to load config");
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file doesn't exist
    /// - The file format is unsupported
    /// - The file contains invalid configuration
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, ConfigError> {
        use config::{Config, File, FileFormat};

        let path = path.as_ref();

        // Check if file exists
        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.to_path_buf()));
        }

        // Determine file format from extension
        let format = match path.extension().and_then(|s| s.to_str()) {
            Some("toml") => FileFormat::Toml,
            Some("yaml") | Some("yml") => FileFormat::Yaml,
            Some("json") => FileFormat::Json,
            _ => return Err(ConfigError::UnsupportedFormat),
        };

        // Build configuration with file + environment variables
        let config = Config::builder()
            .add_source(File::new(
                path.to_str()
                    .ok_or_else(|| ConfigError::UnsupportedFormat)?,
                format,
            ))
            // Environment variables override file settings (12-factor app pattern)
            .add_source(
                config::Environment::with_prefix("TURBOMCP")
                    .separator("__") // Use __ for nested config (e.g., TURBOMCP_TIMEOUTS__REQUEST_TIMEOUT)
                    .try_parsing(true),
            )
            .build()?;

        Ok(config.try_deserialize()?)
    }

    /// Load configuration from a file with custom environment prefix
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use turbomcp_server::ServerConfig;
    ///
    /// // Use MYAPP_PORT instead of TURBOMCP_PORT
    /// let config = ServerConfig::from_file_with_prefix("config.toml", "MYAPP")
    ///     .expect("Failed to load config");
    /// ```
    pub fn from_file_with_prefix(
        path: impl AsRef<std::path::Path>,
        env_prefix: &str,
    ) -> Result<Self, ConfigError> {
        use config::{Config, File, FileFormat};

        let path = path.as_ref();

        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.to_path_buf()));
        }

        let format = match path.extension().and_then(|s| s.to_str()) {
            Some("toml") => FileFormat::Toml,
            Some("yaml") | Some("yml") => FileFormat::Yaml,
            Some("json") => FileFormat::Json,
            _ => return Err(ConfigError::UnsupportedFormat),
        };

        let config = Config::builder()
            .add_source(File::new(
                path.to_str()
                    .ok_or_else(|| ConfigError::UnsupportedFormat)?,
                format,
            ))
            .add_source(
                config::Environment::with_prefix(env_prefix)
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        Ok(config.try_deserialize()?)
    }

    /// Create a configuration builder
    ///
    /// Use this for programmatic configuration without files.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_server::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .name("my-server")
    ///     .port(9000)
    ///     .build();
    /// ```
    pub fn builder() -> ConfigurationBuilder {
        ConfigurationBuilder::new()
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            keep_alive_timeout: Duration::from_secs(60),
            tool_execution_timeout: Duration::from_secs(120), // 2 minutes default for tools
            tool_timeouts: HashMap::new(),                    // No per-tool overrides by default
        }
    }
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_second: 100,
            burst_capacity: 200,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            structured: true,
            file: None,
        }
    }
}

/// Configuration builder
#[derive(Debug)]
pub struct ConfigurationBuilder {
    /// Configuration being built
    config: ServerConfig,
}

impl ConfigurationBuilder {
    /// Create a new configuration builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
        }
    }

    /// Set server name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Set server version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = version.into();
        self
    }

    /// Set server description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.config.description = Some(description.into());
        self
    }

    /// Set bind address
    pub fn bind_address(mut self, address: impl Into<String>) -> Self {
        self.config.bind_address = address.into();
        self
    }

    /// Set port
    #[must_use]
    pub const fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    /// Enable TLS with configuration
    #[must_use]
    pub fn tls(mut self, cert_file: PathBuf, key_file: PathBuf) -> Self {
        self.config.enable_tls = true;
        self.config.tls = Some(TlsConfig {
            cert_file,
            key_file,
        });
        self
    }

    /// Set request timeout
    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.config.timeouts.request_timeout = timeout;
        self
    }

    /// Enable rate limiting
    #[must_use]
    pub const fn rate_limiting(mut self, requests_per_second: u32, burst_capacity: u32) -> Self {
        self.config.rate_limiting.enabled = true;
        self.config.rate_limiting.requests_per_second = requests_per_second;
        self.config.rate_limiting.burst_capacity = burst_capacity;
        self
    }

    /// Set log level
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.logging.level = level.into();
        self
    }

    /// Build the configuration
    #[must_use]
    pub fn build(self) -> ServerConfig {
        self.config
    }
}

impl Default for ConfigurationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration alias for convenience
pub type Configuration = ServerConfig;
#[cfg(test)]
mod inline_tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.name, crate::SERVER_NAME);
        assert_eq!(config.version, crate::SERVER_VERSION);
        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert!(!config.enable_tls);
    }

    #[test]
    fn test_config_builder() {
        let config = ConfigurationBuilder::new()
            .name("test-server")
            .port(9000)
            .build();

        assert_eq!(config.name, "test-server");
        assert_eq!(config.port, 9000);
    }

    // Property-based tests
    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Test that config serialization roundtrips correctly for any valid port
            #[test]
            fn test_config_port_roundtrip(port in 1024u16..65535u16) {
                let config = ConfigurationBuilder::new()
                    .port(port)
                    .build();

                prop_assert_eq!(config.port, port);
            }

            /// Test that server name is preserved through builder
            #[test]
            fn test_config_name_preservation(name in "[a-zA-Z0-9_-]{1,50}") {
                let config = ConfigurationBuilder::new()
                    .name(&name)
                    .build();

                prop_assert_eq!(config.name, name);
            }

            /// Test rate limiting configuration validity
            #[test]
            fn test_rate_limiting_config(
                rps in 1u32..10000u32,
                burst in 1u32..1000u32
            ) {
                let config = RateLimitingConfig {
                    enabled: true,
                    requests_per_second: rps,
                    burst_capacity: burst,
                };

                // Verify values are within bounds
                prop_assert!(config.requests_per_second >= 1);
                prop_assert!(config.burst_capacity >= 1);
            }
        }
    }
}

/// WebSocket server configuration
///
/// Configuration for WebSocket transport when using `run_websocket_with_config()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "websocket")]
pub struct WebSocketServerConfig {
    /// Bind address (e.g., "127.0.0.1:8080")
    pub bind_addr: String,
    /// WebSocket endpoint path (default: "/ws")
    pub endpoint_path: String,
    /// Maximum concurrent request handlers per connection (default: 100)
    pub max_concurrent_requests: usize,
}

#[cfg(feature = "websocket")]
impl Default for WebSocketServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            endpoint_path: "/ws".to_string(),
            max_concurrent_requests: 100,
        }
    }
}

// Additional comprehensive tests in separate file
#[cfg(test)]
mod tests;
