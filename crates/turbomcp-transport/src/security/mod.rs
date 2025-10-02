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
//! use turbomcp_transport::security::{SecurityValidator, SecurityConfigBuilder};
//! use std::collections::HashMap;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a security validator for production
//! let validator = SecurityConfigBuilder::for_production()
//!     .with_allowed_origins(vec!["https://app.example.com".to_string()])
//!     .with_api_keys(vec!["your-secret-api-key".to_string()])
//!     .with_rate_limit(100, std::time::Duration::from_secs(60))
//!     .build();
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
//! ## Enhanced Security with Session Management
//!
//! ```rust,no_run
//! use turbomcp_transport::security::{EnhancedSecurityConfigBuilder, SessionSecurityManager};
//! use std::time::Duration;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create enhanced security with session management
//! let (validator, session_manager) = EnhancedSecurityConfigBuilder::for_production()
//!     .with_allowed_origins(vec!["https://app.example.com".to_string()])
//!     .with_api_keys(vec!["api-key".to_string()])
//!     .with_max_sessions_per_ip(5)
//!     .with_session_idle_timeout(Duration::from_secs(15 * 60))
//!     .enforce_ip_binding(true)
//!     .build();
//!
//! // Create and validate sessions
//! let client_ip = "192.168.1.100".parse()?;
//! let session = session_manager.create_session(client_ip, Some("Mozilla/5.0"))?;
//! println!("Created session: {}", session.id);
//! # Ok(())
//! # }
//! ```
//!
//! ## Environment-Specific Configurations
//!
//! ```rust,no_run
//! use turbomcp_transport::security::{SecurityConfigBuilder, OriginConfig, AuthConfig};
//!
//! # fn example() {
//! // Development - relaxed security for ease of development
//! let dev_validator = SecurityConfigBuilder::for_development().build();
//!
//! // Production - strict security
//! let prod_validator = SecurityConfigBuilder::for_production()
//!     .with_allowed_origins(vec!["https://prod.example.com".to_string()])
//!     .with_api_keys(vec!["prod-secret-key".to_string()])
//!     .build();
//!
//! // Testing - minimal security for fast tests
//! let test_validator = SecurityConfigBuilder::for_testing().build();
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

/// Presets for common security configurations
pub mod presets {
    use super::*;
    use std::time::Duration;

    /// High-security preset for critical production systems
    pub fn high_security() -> (
        OriginConfig,
        AuthConfig,
        RateLimitConfig,
        SessionSecurityConfig,
    ) {
        (
            OriginConfig {
                allowed_origins: std::collections::HashSet::new(),
                allow_localhost: false,
                allow_any: false,
            },
            AuthConfig {
                require_auth: true,
                api_keys: std::collections::HashSet::new(),
                method: AuthMethod::Bearer,
            },
            RateLimitConfig {
                max_requests: 50,
                window: Duration::from_secs(60),
                enabled: true,
            },
            SessionSecurityConfig {
                max_lifetime: Duration::from_secs(4 * 60 * 60), // 4 hours
                idle_timeout: Duration::from_secs(10 * 60),     // 10 minutes
                max_sessions_per_ip: 3,
                enforce_ip_binding: true,
                regenerate_session_ids: true,
                regeneration_interval: Duration::from_secs(15 * 60), // 15 minutes
            },
        )
    }

    /// Balanced preset for typical production use
    pub fn balanced() -> (
        OriginConfig,
        AuthConfig,
        RateLimitConfig,
        SessionSecurityConfig,
    ) {
        (
            OriginConfig {
                allowed_origins: std::collections::HashSet::new(),
                allow_localhost: false,
                allow_any: false,
            },
            AuthConfig {
                require_auth: true,
                api_keys: std::collections::HashSet::new(),
                method: AuthMethod::Bearer,
            },
            RateLimitConfig {
                max_requests: 100,
                window: Duration::from_secs(60),
                enabled: true,
            },
            SessionSecurityConfig {
                max_lifetime: Duration::from_secs(8 * 60 * 60), // 8 hours
                idle_timeout: Duration::from_secs(30 * 60),     // 30 minutes
                max_sessions_per_ip: 5,
                enforce_ip_binding: true,
                regenerate_session_ids: true,
                regeneration_interval: Duration::from_secs(60 * 60), // 1 hour
            },
        )
    }

    /// Relaxed preset for development environments
    pub fn relaxed() -> (
        OriginConfig,
        AuthConfig,
        RateLimitConfig,
        SessionSecurityConfig,
    ) {
        (
            OriginConfig {
                allowed_origins: std::collections::HashSet::new(),
                allow_localhost: true,
                allow_any: false,
            },
            AuthConfig {
                require_auth: false,
                api_keys: std::collections::HashSet::new(),
                method: AuthMethod::Bearer,
            },
            RateLimitConfig {
                max_requests: 1000,
                window: Duration::from_secs(60),
                enabled: false,
            },
            SessionSecurityConfig {
                max_lifetime: Duration::from_secs(24 * 60 * 60), // 24 hours
                idle_timeout: Duration::from_secs(60 * 60),      // 1 hour
                max_sessions_per_ip: 100,
                enforce_ip_binding: false,
                regenerate_session_ids: false,
                regeneration_interval: Duration::from_secs(24 * 60 * 60),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presets_compilation() {
        // Test that all presets compile and can be used
        let (origin, auth, rate_limit, session) = presets::high_security();
        assert!(!origin.allow_localhost);
        assert!(auth.require_auth);
        assert!(rate_limit.enabled);
        assert!(session.enforce_ip_binding);

        let (origin, auth, rate_limit, session) = presets::balanced();
        assert!(!origin.allow_localhost);
        assert!(auth.require_auth);
        assert!(rate_limit.enabled);
        assert!(session.enforce_ip_binding);

        let (origin, auth, rate_limit, session) = presets::relaxed();
        assert!(origin.allow_localhost);
        assert!(!auth.require_auth);
        assert!(!rate_limit.enabled);
        assert!(!session.enforce_ip_binding);
    }

    #[test]
    fn test_comprehensive_example() {
        use std::collections::HashMap;

        // Test the example from the module documentation
        let validator = SecurityConfigBuilder::for_testing().build();

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
        let (_validator, session_manager) = EnhancedSecurityConfigBuilder::for_testing().build();

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
