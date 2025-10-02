//! Authentication framework for transport layer
//!
//! This module provides flexible authentication mechanisms including
//! Bearer tokens, API keys, and custom header authentication.
//! Supports multiple authentication methods and environments.

use super::errors::SecurityError;
use crate::security::SecurityHeaders;
use std::collections::HashSet;

/// Authentication methods
#[derive(Clone, Debug)]
pub enum AuthMethod {
    /// Bearer token authentication
    Bearer,
    /// API key in Authorization header
    ApiKey,
    /// Custom header authentication
    Custom(String),
}

/// Authentication configuration
#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// Whether authentication is required
    pub require_auth: bool,
    /// Valid API keys for authentication
    pub api_keys: HashSet<String>,
    /// Authentication method
    pub method: AuthMethod,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            require_auth: false,
            api_keys: HashSet::new(),
            method: AuthMethod::Bearer,
        }
    }
}

impl AuthConfig {
    /// Create a new authentication configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create development configuration with no authentication required
    pub fn for_development() -> Self {
        Self {
            require_auth: false,
            api_keys: HashSet::new(),
            method: AuthMethod::Bearer,
        }
    }

    /// Create production configuration with required authentication
    pub fn for_production(api_keys: Vec<String>, method: AuthMethod) -> Self {
        Self {
            require_auth: true,
            api_keys: api_keys.into_iter().collect(),
            method,
        }
    }

    /// Create testing configuration with a test API key
    pub fn for_testing() -> Self {
        let mut api_keys = HashSet::new();
        api_keys.insert("test-api-key".to_string());

        Self {
            require_auth: true,
            api_keys,
            method: AuthMethod::Bearer,
        }
    }

    /// Add an API key
    pub fn add_api_key(&mut self, key: String) {
        self.api_keys.insert(key);
    }

    /// Add multiple API keys
    pub fn add_api_keys(&mut self, keys: Vec<String>) {
        self.api_keys.extend(keys);
    }

    /// Set whether authentication is required
    pub fn set_require_auth(&mut self, require: bool) {
        self.require_auth = require;
    }

    /// Set the authentication method
    pub fn set_method(&mut self, method: AuthMethod) {
        self.method = method;
    }

    /// Check if API key is valid
    pub fn is_valid_key(&self, key: &str) -> bool {
        self.api_keys.contains(key)
    }
}

/// Validate authentication credentials
pub fn validate_authentication(
    config: &AuthConfig,
    headers: &SecurityHeaders,
) -> Result<(), SecurityError> {
    if !config.require_auth {
        return Ok(());
    }

    match config.method {
        AuthMethod::Bearer => {
            let auth_header = headers.get("Authorization").ok_or_else(|| {
                SecurityError::AuthenticationFailed("Missing Authorization header".to_string())
            })?;
            validate_bearer_token(config, auth_header)
        }
        AuthMethod::ApiKey => {
            let auth_header = headers.get("Authorization").ok_or_else(|| {
                SecurityError::AuthenticationFailed("Missing Authorization header".to_string())
            })?;
            validate_api_key(config, auth_header)
        }
        AuthMethod::Custom(ref header_name) => validate_custom_header(config, headers, header_name),
    }
}

/// Validate Bearer token authentication
fn validate_bearer_token(config: &AuthConfig, auth_header: &str) -> Result<(), SecurityError> {
    if !auth_header.starts_with("Bearer ") {
        return Err(SecurityError::AuthenticationFailed(
            "Invalid Authorization format, expected Bearer token".to_string(),
        ));
    }

    let token = &auth_header[7..];
    if !config.api_keys.contains(token) {
        return Err(SecurityError::AuthenticationFailed(
            "Invalid bearer token".to_string(),
        ));
    }

    Ok(())
}

/// Validate API key authentication
fn validate_api_key(config: &AuthConfig, auth_header: &str) -> Result<(), SecurityError> {
    if !auth_header.starts_with("ApiKey ") {
        return Err(SecurityError::AuthenticationFailed(
            "Invalid Authorization format, expected ApiKey".to_string(),
        ));
    }

    let key = &auth_header[7..];
    if !config.api_keys.contains(key) {
        return Err(SecurityError::AuthenticationFailed(
            "Invalid API key".to_string(),
        ));
    }

    Ok(())
}

/// Validate custom header authentication
fn validate_custom_header(
    config: &AuthConfig,
    headers: &SecurityHeaders,
    header_name: &str,
) -> Result<(), SecurityError> {
    let custom_value = headers.get(header_name).ok_or_else(|| {
        SecurityError::AuthenticationFailed(format!("Missing {} header", header_name))
    })?;

    if !config.api_keys.contains(custom_value) {
        return Err(SecurityError::AuthenticationFailed(format!(
            "Invalid {} value",
            header_name
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert!(!config.require_auth);
        assert!(config.api_keys.is_empty());
        assert!(matches!(config.method, AuthMethod::Bearer));
    }

    #[test]
    fn test_auth_config_for_development() {
        let config = AuthConfig::for_development();
        assert!(!config.require_auth);
    }

    #[test]
    fn test_auth_config_for_production() {
        let keys = vec!["secret123".to_string()];
        let config = AuthConfig::for_production(keys.clone(), AuthMethod::Bearer);
        assert!(config.require_auth);
        assert!(config.api_keys.contains(&keys[0]));
    }

    #[test]
    fn test_bearer_authentication_success() {
        let config = AuthConfig::for_production(vec!["secret123".to_string()], AuthMethod::Bearer);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer secret123".to_string());

        assert!(validate_authentication(&config, &headers).is_ok());
    }

    #[test]
    fn test_bearer_authentication_invalid_token() {
        let config = AuthConfig::for_production(vec!["secret123".to_string()], AuthMethod::Bearer);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer wrong".to_string());

        assert!(validate_authentication(&config, &headers).is_err());
    }

    #[test]
    fn test_bearer_authentication_invalid_format() {
        let config = AuthConfig::for_production(vec!["secret123".to_string()], AuthMethod::Bearer);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Basic secret123".to_string());

        assert!(validate_authentication(&config, &headers).is_err());
    }

    #[test]
    fn test_api_key_authentication() {
        let config = AuthConfig::for_production(vec!["api123".to_string()], AuthMethod::ApiKey);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "ApiKey api123".to_string());

        assert!(validate_authentication(&config, &headers).is_ok());
    }

    #[test]
    fn test_custom_header_authentication() {
        let config = AuthConfig::for_production(
            vec!["custom123".to_string()],
            AuthMethod::Custom("X-API-Key".to_string()),
        );

        let mut headers = HashMap::new();
        headers.insert("X-API-Key".to_string(), "custom123".to_string());

        assert!(validate_authentication(&config, &headers).is_ok());
    }

    #[test]
    fn test_no_auth_required() {
        let config = AuthConfig::for_development();
        let headers = HashMap::new();

        assert!(validate_authentication(&config, &headers).is_ok());
    }

    #[test]
    fn test_missing_authorization_header() {
        let config = AuthConfig::for_production(vec!["secret".to_string()], AuthMethod::Bearer);
        let headers = HashMap::new();

        assert!(validate_authentication(&config, &headers).is_err());
    }
}
