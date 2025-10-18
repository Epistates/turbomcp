//! Authentication configuration management
//!
//! This module provides authentication configuration for various
//! authentication methods including JWT and API keys.

/// Authentication configuration for middleware
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Enable authentication
    pub enabled: bool,
    /// JWT secret for token validation
    pub jwt_secret: Option<String>,
    /// API key header name
    pub api_key_header: Option<String>,
    /// Custom authentication provider
    pub custom_validator: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            jwt_secret: None,
            api_key_header: Some("x-api-key".to_string()),
            custom_validator: None,
        }
    }
}

impl AuthConfig {
    /// Create new authentication config with JWT
    pub fn jwt(secret: String) -> Self {
        Self {
            enabled: true,
            jwt_secret: Some(secret),
            api_key_header: None,
            custom_validator: None,
        }
    }

    /// Create new authentication config with API key
    pub fn api_key(header: String) -> Self {
        Self {
            enabled: true,
            jwt_secret: None,
            api_key_header: Some(header),
            custom_validator: None,
        }
    }

    /// Create new authentication config with custom validator
    pub fn custom(validator: String) -> Self {
        Self {
            enabled: true,
            jwt_secret: None,
            api_key_header: None,
            custom_validator: Some(validator),
        }
    }

    /// Disable authentication
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            jwt_secret: None,
            api_key_header: None,
            custom_validator: None,
        }
    }
}
