//! Server configuration management
//!
//! This module provides the main `McpServerConfig` struct with environment-specific
//! presets and builder pattern methods for configuring the MCP server.

use std::time::Duration;

use super::{
    AuthConfig, CorsConfig, Environment, RateLimitConfig, SecurityConfig, TlsConfig, TlsVersion,
};

/// Production-grade configuration for MCP server with comprehensive production settings
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Maximum request size in bytes
    pub max_request_size: usize,

    /// Request timeout duration
    pub request_timeout: Duration,

    /// SSE keep-alive interval
    pub sse_keep_alive: Duration,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// CORS configuration
    pub cors: CorsConfig,

    /// Security headers configuration
    pub security: SecurityConfig,

    /// Rate limiting configuration
    pub rate_limiting: RateLimitConfig,

    /// TLS configuration
    pub tls: Option<TlsConfig>,

    /// Authentication configuration
    pub auth: Option<AuthConfig>,

    /// Enable compression
    pub enable_compression: bool,

    /// Enable request tracing
    pub enable_tracing: bool,

    /// Environment mode (Development, Staging, Production)
    pub environment: Environment,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self::development()
    }
}

impl McpServerConfig {
    /// Create development configuration with permissive settings
    pub fn development() -> Self {
        Self {
            max_request_size: 16 * 1024 * 1024, // 16MB
            request_timeout: Duration::from_secs(30),
            sse_keep_alive: Duration::from_secs(15),
            max_connections: 1000,
            cors: CorsConfig::permissive(),
            security: SecurityConfig::development(),
            rate_limiting: RateLimitConfig::disabled(),
            tls: None,
            auth: None,
            enable_compression: true,
            enable_tracing: true,
            environment: Environment::Development,
        }
    }

    /// Create staging configuration with moderate security
    pub fn staging() -> Self {
        Self {
            max_request_size: 8 * 1024 * 1024, // 8MB
            request_timeout: Duration::from_secs(30),
            sse_keep_alive: Duration::from_secs(15),
            max_connections: 500,
            cors: CorsConfig::restrictive(),
            security: SecurityConfig::staging(),
            rate_limiting: RateLimitConfig::moderate(),
            tls: Self::load_tls_from_env(),
            auth: Self::load_auth_from_env(),
            enable_compression: true,
            enable_tracing: true,
            environment: Environment::Staging,
        }
    }

    /// Create production configuration with strict security
    pub fn production() -> Self {
        Self {
            max_request_size: 4 * 1024 * 1024, // 4MB
            request_timeout: Duration::from_secs(15),
            sse_keep_alive: Duration::from_secs(30),
            max_connections: 200,
            cors: CorsConfig::strict(),
            security: SecurityConfig::production(),
            rate_limiting: RateLimitConfig::strict(),
            tls: Self::load_tls_from_env(),
            auth: Self::load_auth_from_env(),
            enable_compression: true,
            enable_tracing: true,
            environment: Environment::Production,
        }
    }

    /// Builder method: Set CORS origins
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors.allowed_origins = Some(origins);
        self
    }

    /// Builder method: Set custom Content Security Policy
    pub fn with_custom_csp(mut self, csp: &str) -> Self {
        self.security.content_security_policy = Some(csp.to_string());
        self
    }

    /// Builder method: Set rate limiting parameters
    pub fn with_rate_limit(mut self, requests_per_minute: u32, burst: u32) -> Self {
        self.rate_limiting.requests_per_minute = requests_per_minute;
        self.rate_limiting.burst_capacity = burst;
        self.rate_limiting.enabled = true;
        self
    }

    /// Builder method: Configure TLS
    pub fn with_tls(mut self, cert_file: String, key_file: String) -> Self {
        self.tls = Some(TlsConfig {
            cert_file,
            key_file,
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        });
        self
    }

    /// Builder method: Configure API key authentication
    pub fn with_api_key_auth(mut self, header_name: String) -> Self {
        self.auth = Some(AuthConfig::api_key(header_name));
        self
    }

    /// Builder method: Configure JWT authentication
    pub fn with_jwt_auth(mut self, secret: String) -> Self {
        self.auth = Some(AuthConfig::jwt(secret));
        self
    }

    /// Load TLS configuration from environment variables
    fn load_tls_from_env() -> Option<TlsConfig> {
        let cert_file = std::env::var("TURBOMCP_TLS_CERT").ok()?;
        let key_file = std::env::var("TURBOMCP_TLS_KEY").ok()?;

        Some(TlsConfig {
            cert_file,
            key_file,
            // Allow TLS 1.2 for backwards compatibility with older clients
            #[allow(deprecated)]
            min_version: std::env::var("TURBOMCP_TLS_MIN_VERSION")
                .ok()
                .and_then(|v| match v.as_str() {
                    "1.2" => Some(TlsVersion::TlsV1_2),
                    "1.3" => Some(TlsVersion::TlsV1_3),
                    _ => None,
                })
                .unwrap_or(TlsVersion::TlsV1_3),
            enable_http2: std::env::var("TURBOMCP_ENABLE_HTTP2")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
        })
    }

    /// Load authentication configuration from environment variables
    fn load_auth_from_env() -> Option<AuthConfig> {
        if let Ok(jwt_secret) = std::env::var("TURBOMCP_JWT_SECRET") {
            return Some(AuthConfig::jwt(jwt_secret));
        }

        if let Ok(api_key_header) = std::env::var("TURBOMCP_API_KEY_HEADER") {
            return Some(AuthConfig::api_key(api_key_header));
        }

        None
    }
}
