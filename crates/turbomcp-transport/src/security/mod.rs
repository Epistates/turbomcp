//! Security module for transport layer
//!
//! This module provides comprehensive security features for MCP transports including:
//! - **Origin validation** to prevent DNS rebinding attacks (MCP spec compliance)
//! - **Authentication framework** with Bearer tokens, API keys, and custom headers
//! - **Rate limiting** with sliding window algorithm to prevent abuse
//! - **Session security** with IP binding, fingerprinting, and automatic expiration
//! - **Message size validation** to prevent DoS attacks
//! - **Security configuration builders** for type-safe, fluent configuration
//!
//! ## Architecture
//!
//! The security module is organized into focused components:
//!
//! ```text
//! security/
//! ├── errors.rs      # Security error types
//! ├── origin.rs      # Origin validation (DNS rebinding protection)
//! ├── auth.rs        # Authentication configuration and validation
//! ├── rate_limit.rs  # Rate limiting with sliding window algorithm
//! ├── session.rs     # Secure session management
//! ├── validator.rs   # Main SecurityValidator coordinating all checks
//! ├── builder.rs     # Configuration builders for type-safe setup
//! └── utils.rs       # Utility functions and common operations
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use turbomcp_transport::security::{SecurityValidator, OriginConfig, AuthConfig, RateLimitConfig};
//! use std::collections::HashMap;
//! use std::time::Duration;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a security validator with explicit configuration
//! let validator = SecurityValidator::new(
//!     OriginConfig {
//!         allowed_origins: vec!["https://app.example.com".to_string()].into_iter().collect(),
//!         allow_localhost: false,
//!         allow_any: false,
//!     },
//!     AuthConfig {
//!         require_auth: true,
//!         api_keys: vec!["your-secret-api-key".to_string()].into_iter().collect(),
//!         method: turbomcp_transport::security::AuthMethod::Bearer,
//!     },
//!     Some(RateLimitConfig {
//!         max_requests: 100,
//!         window: Duration::from_secs(60),
//!         enabled: true,
//!     }),
//! );
//!
//! // Validate a request
//! let mut headers = HashMap::new();
//! headers.insert("Origin".to_string(), "https://app.example.com".to_string());
//! headers.insert("Authorization".to_string(), "Bearer your-secret-api-key".to_string());
//!
//! let client_ip = "192.168.1.100".parse()?;
//! validator.validate_request(&headers, client_ip)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Using Builders for Convenience
//!
//! ```rust,no_run
//! use turbomcp_transport::security::SecurityConfigBuilder;
//! use std::time::Duration;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create security validator using builder pattern
//! let validator = SecurityConfigBuilder::new()
//!     .with_allowed_origins(vec!["https://app.example.com".to_string()])
//!     .with_api_keys(vec!["api-key".to_string()])
//!     .require_authentication(true)
//!     .with_rate_limit(100, Duration::from_secs(60))
//!     .build();
//!
//! // Or use defaults and customize specific settings
//! let dev_validator = SecurityConfigBuilder::new()
//!     .allow_localhost(true)
//!     .disable_rate_limiting()
//!     .build();
//! # Ok(())
//! # }
//! ```

pub mod auth;
pub mod builder;
pub mod errors;
pub mod origin;
pub mod rate_limit;
pub mod session;
pub mod utils;
pub mod validator;

// Re-export all main types for convenience
pub use auth::{AuthConfig, AuthMethod, validate_authentication};
pub use builder::{EnhancedSecurityConfigBuilder, SecurityConfigBuilder};
pub use errors::SecurityError;
pub use origin::{OriginConfig, validate_origin};
pub use rate_limit::{RateLimitConfig, RateLimiter, check_rate_limit};
pub use session::{SecureSessionInfo, SessionSecurityConfig, SessionSecurityManager};
pub use utils::{
    HeaderValue, SecurityHeaders, create_cors_headers, create_security_headers, extract_api_key,
    extract_bearer_token, extract_client_ip, generate_secure_token, is_localhost_origin,
    is_safe_header_value, sanitize_header_value, size_limits, validate_json_size,
    validate_message_size, validate_string_size,
};
pub use validator::SecurityValidator;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comprehensive_example() {
        use std::collections::HashMap;

        // Test the example from the module documentation
        let validator = SecurityConfigBuilder::new()
            .allow_localhost(true)
            .allow_any_origin(true)
            .require_authentication(true)
            .with_api_keys(vec!["test-api-key".to_string()])
            .with_rate_limit(2, std::time::Duration::from_secs(1))
            .build();

        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        headers.insert(
            "Authorization".to_string(),
            "Bearer test-api-key".to_string(),
        );

        let client_ip = "127.0.0.1".parse().unwrap();

        // Should validate successfully with testing configuration
        assert!(validator.validate_request(&headers, client_ip).is_ok());
    }

    #[test]
    fn test_enhanced_security_example() {
        // Test enhanced security configuration
        let (_validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .with_security_config(
                SecurityConfigBuilder::new()
                    .allow_localhost(true)
                    .allow_any_origin(true),
            )
            .with_max_sessions_per_ip(2)
            .build();

        let client_ip = "127.0.0.1".parse().unwrap();
        let session = session_manager
            .create_session(client_ip, Some("test-agent"))
            .unwrap();

        assert!(session.id.starts_with("mcp_session_"));
        assert_eq!(session.original_ip, client_ip);

        // Should be able to validate the session
        let validated = session_manager
            .validate_session(&session.id, client_ip, Some("test-agent"))
            .unwrap();
        assert_eq!(validated.request_count, 1);
    }
}
