//! Main security validator for coordinating all security checks
//!
//! This module provides the SecurityValidator that combines origin validation,
//! authentication, and rate limiting into a single interface for comprehensive
//! security validation of HTTP requests.

use super::auth::{AuthConfig, validate_authentication};
use super::errors::SecurityError;
use super::origin::{OriginConfig, validate_origin};
use super::rate_limit::{RateLimitConfig, RateLimiter};
use crate::security::SecurityHeaders;
use std::net::IpAddr;

/// Security validator for HTTP requests
#[derive(Debug)]
pub struct SecurityValidator {
    origin_config: OriginConfig,
    auth_config: AuthConfig,
    rate_limiter: Option<RateLimiter>,
}

impl SecurityValidator {
    /// Create a new security validator
    pub fn new(
        origin_config: OriginConfig,
        auth_config: AuthConfig,
        rate_limit_config: Option<RateLimitConfig>,
    ) -> Self {
        let rate_limiter = rate_limit_config.map(RateLimiter::new);

        Self {
            origin_config,
            auth_config,
            rate_limiter,
        }
    }

    /// Validate Origin header to prevent DNS rebinding attacks
    ///
    /// Per MCP 2025-06-18 specification:
    /// "Servers MUST validate the Origin header on all incoming connections
    /// to prevent DNS rebinding attacks"
    ///
    /// Smart localhost handling: localhost→localhost connections without Origin are allowed
    /// (DNS rebinding attacks require remote origins, so localhost clients are inherently safe)
    pub fn validate_origin(
        &self,
        headers: &SecurityHeaders,
        client_ip: IpAddr,
    ) -> Result<(), SecurityError> {
        validate_origin(&self.origin_config, headers, client_ip)
    }

    /// Validate authentication credentials
    pub fn validate_authentication(&self, headers: &SecurityHeaders) -> Result<(), SecurityError> {
        validate_authentication(&self.auth_config, headers)
    }

    /// Check rate limits for a client IP
    pub fn check_rate_limit(&self, client_ip: IpAddr) -> Result<(), SecurityError> {
        if let Some(ref rate_limiter) = self.rate_limiter {
            rate_limiter.check_rate_limit(client_ip)?;
        }
        Ok(())
    }

    /// Comprehensive security validation for HTTP requests
    pub fn validate_request(
        &self,
        headers: &SecurityHeaders,
        client_ip: IpAddr,
    ) -> Result<(), SecurityError> {
        // 1. Validate Origin header (DNS rebinding protection with smart localhost handling)
        self.validate_origin(headers, client_ip)?;

        // 2. Validate authentication
        self.validate_authentication(headers)?;

        // 3. Check rate limits
        self.check_rate_limit(client_ip)?;

        Ok(())
    }

    /// Get origin configuration
    pub fn origin_config(&self) -> &OriginConfig {
        &self.origin_config
    }

    /// Get authentication configuration
    pub fn auth_config(&self) -> &AuthConfig {
        &self.auth_config
    }

    /// Get rate limiter reference
    pub fn rate_limiter(&self) -> Option<&RateLimiter> {
        self.rate_limiter.as_ref()
    }

    /// Update origin configuration
    pub fn set_origin_config(&mut self, config: OriginConfig) {
        self.origin_config = config;
    }

    /// Update authentication configuration
    pub fn set_auth_config(&mut self, config: AuthConfig) {
        self.auth_config = config;
    }

    /// Update rate limiter
    pub fn set_rate_limiter(&mut self, rate_limiter: Option<RateLimiter>) {
        self.rate_limiter = rate_limiter;
    }

    /// Get current request count for a client (if rate limiting enabled)
    pub fn get_request_count(&self, client_ip: IpAddr) -> usize {
        self.rate_limiter
            .as_ref()
            .map(|limiter| limiter.get_request_count(client_ip))
            .unwrap_or(0)
    }

    /// Get remaining requests for a client (if rate limiting enabled)
    pub fn get_remaining_requests(&self, client_ip: IpAddr) -> usize {
        self.rate_limiter
            .as_ref()
            .map(|limiter| limiter.get_remaining_requests(client_ip))
            .unwrap_or(usize::MAX)
    }

    /// Clean up expired rate limit entries
    pub fn cleanup_expired(&self) -> usize {
        self.rate_limiter
            .as_ref()
            .map(|limiter| limiter.cleanup_expired())
            .unwrap_or(0)
    }

    /// Get total number of tracked clients (if rate limiting enabled)
    pub fn client_count(&self) -> usize {
        self.rate_limiter
            .as_ref()
            .map(|limiter| limiter.client_count())
            .unwrap_or(0)
    }
}

impl Default for SecurityValidator {
    fn default() -> Self {
        Self::new(
            OriginConfig::default(),
            AuthConfig::default(),
            Some(RateLimitConfig::default()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::AuthMethod;
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn test_security_validator_default() {
        let validator = SecurityValidator::default();

        // Should have default configurations
        assert!(validator.origin_config().allow_localhost);
        assert!(!validator.auth_config().require_auth);
        assert!(validator.rate_limiter().is_some());
    }

    #[test]
    fn test_comprehensive_validation_success() {
        let validator = SecurityValidator::new(
            OriginConfig {
                allow_localhost: true,
                allow_any: true,
                ..Default::default()
            },
            AuthConfig {
                require_auth: true,
                api_keys: vec!["test-api-key".to_string()].into_iter().collect(),
                method: AuthMethod::Bearer,
            },
            Some(RateLimitConfig {
                max_requests: 2,
                window: Duration::from_secs(1),
                enabled: true,
            }),
        );
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        headers.insert(
            "Authorization".to_string(),
            "Bearer test-api-key".to_string(),
        );

        let client_ip = "127.0.0.1".parse().unwrap();

        assert!(validator.validate_request(&headers, client_ip).is_ok());
    }

    #[test]
    fn test_comprehensive_validation_origin_failure() {
        let validator = SecurityValidator::new(
            OriginConfig {
                allowed_origins: vec!["https://trusted.com".to_string()]
                    .into_iter()
                    .collect(),
                allow_localhost: false,
                allow_any: false,
            },
            AuthConfig {
                require_auth: true,
                api_keys: vec!["secret".to_string()].into_iter().collect(),
                method: AuthMethod::Bearer,
            },
            Some(RateLimitConfig {
                max_requests: 100,
                window: Duration::from_secs(60),
                enabled: true,
            }),
        );

        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://evil.com".to_string());
        headers.insert("Authorization".to_string(), "Bearer secret".to_string());

        let client_ip = "127.0.0.1".parse().unwrap();

        assert!(validator.validate_request(&headers, client_ip).is_err());
    }

    #[test]
    fn test_comprehensive_validation_auth_failure() {
        let validator = SecurityValidator::new(
            OriginConfig {
                allowed_origins: vec!["https://trusted.com".to_string()]
                    .into_iter()
                    .collect(),
                allow_localhost: false,
                allow_any: false,
            },
            AuthConfig {
                require_auth: true,
                api_keys: vec!["secret".to_string()].into_iter().collect(),
                method: AuthMethod::Bearer,
            },
            Some(RateLimitConfig {
                max_requests: 100,
                window: Duration::from_secs(60),
                enabled: true,
            }),
        );

        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "https://trusted.com".to_string());
        headers.insert("Authorization".to_string(), "Bearer wrong".to_string());

        let client_ip = "127.0.0.1".parse().unwrap();

        assert!(validator.validate_request(&headers, client_ip).is_err());
    }

    #[test]
    fn test_comprehensive_validation_rate_limit_failure() {
        let validator = SecurityValidator::new(
            OriginConfig {
                allow_localhost: true,
                allow_any: true,
                ..Default::default()
            },
            AuthConfig {
                require_auth: true,
                api_keys: vec!["test-api-key".to_string()].into_iter().collect(),
                method: AuthMethod::Bearer,
            },
            Some(RateLimitConfig {
                max_requests: 2,
                window: Duration::from_secs(1),
                enabled: true,
            }),
        );
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        headers.insert(
            "Authorization".to_string(),
            "Bearer test-api-key".to_string(),
        );

        let client_ip = "127.0.0.1".parse().unwrap();

        // First request should succeed
        assert!(validator.validate_request(&headers, client_ip).is_ok());

        // Second request should succeed (limit is 2 for testing)
        assert!(validator.validate_request(&headers, client_ip).is_ok());

        // Third request should fail due to rate limiting
        assert!(validator.validate_request(&headers, client_ip).is_err());
    }

    #[test]
    fn test_individual_validation_methods() {
        let validator = SecurityValidator::new(
            OriginConfig {
                allow_localhost: true,
                allow_any: true,
                ..Default::default()
            },
            AuthConfig {
                require_auth: true,
                api_keys: vec!["test-api-key".to_string()].into_iter().collect(),
                method: AuthMethod::Bearer,
            },
            Some(RateLimitConfig {
                max_requests: 2,
                window: Duration::from_secs(1),
                enabled: true,
            }),
        );
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        headers.insert(
            "Authorization".to_string(),
            "Bearer test-api-key".to_string(),
        );

        let client_ip = "127.0.0.1".parse().unwrap();

        // Test individual validation methods
        assert!(validator.validate_origin(&headers, client_ip).is_ok());
        assert!(validator.validate_authentication(&headers).is_ok());
        assert!(validator.check_rate_limit(client_ip).is_ok());
    }

    #[test]
    fn test_request_count_tracking() {
        let validator = SecurityValidator::new(
            OriginConfig {
                allow_localhost: true,
                allow_any: true,
                ..Default::default()
            },
            AuthConfig {
                require_auth: true,
                api_keys: vec!["test-api-key".to_string()].into_iter().collect(),
                method: AuthMethod::Bearer,
            },
            Some(RateLimitConfig {
                max_requests: 2,
                window: Duration::from_secs(1),
                enabled: true,
            }),
        );
        let client_ip = "127.0.0.1".parse().unwrap();

        // Initial count should be 0
        assert_eq!(validator.get_request_count(client_ip), 0);

        // Make a request
        validator.check_rate_limit(client_ip).unwrap();
        assert_eq!(validator.get_request_count(client_ip), 1);

        // Check remaining requests
        let remaining = validator.get_remaining_requests(client_ip);
        assert_eq!(remaining, 1); // Testing config has limit of 2
    }

    #[test]
    fn test_config_updates() {
        let mut validator = SecurityValidator::default();

        // Update configurations
        let new_origin_config = OriginConfig {
            allowed_origins: vec!["https://new.com".to_string()].into_iter().collect(),
            allow_localhost: false,
            allow_any: false,
        };
        let new_auth_config = AuthConfig {
            require_auth: true,
            api_keys: vec!["newkey".to_string()].into_iter().collect(),
            method: AuthMethod::ApiKey,
        };

        validator.set_origin_config(new_origin_config);
        validator.set_auth_config(new_auth_config);

        // Verify updates
        assert!(!validator.origin_config().allow_localhost);
        assert!(validator.auth_config().require_auth);
        assert!(validator.auth_config().api_keys.contains("newkey"));
    }
}
