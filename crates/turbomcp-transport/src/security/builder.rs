//! Configuration builders for security components
//!
//! This module provides builder patterns for creating security configurations
//! in a fluent, type-safe manner. Includes builders for both basic security
//! validation and enhanced security with session management.

use super::auth::{AuthConfig, AuthMethod};
use super::origin::OriginConfig;
use super::rate_limit::RateLimitConfig;
use super::session::{SessionSecurityConfig, SessionSecurityManager};
use super::validator::SecurityValidator;
use std::time::Duration;

/// Security configuration builder
#[derive(Debug)]
pub struct SecurityConfigBuilder {
    origin_config: OriginConfig,
    auth_config: AuthConfig,
    rate_limit_config: Option<RateLimitConfig>,
}

impl Default for SecurityConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityConfigBuilder {
    /// Create a new security configuration builder
    pub fn new() -> Self {
        Self {
            origin_config: OriginConfig::default(),
            auth_config: AuthConfig::default(),
            rate_limit_config: Some(RateLimitConfig::default()),
        }
    }

    /// Create builder for development environment
    pub fn for_development() -> Self {
        Self {
            origin_config: OriginConfig::for_development(),
            auth_config: AuthConfig::for_development(),
            rate_limit_config: Some(RateLimitConfig::for_development()),
        }
    }

    /// Create builder for production environment - requires explicit configuration
    ///
    /// This returns a builder that needs origins and API keys configured.
    /// Use builder methods to complete configuration with explicit, named parameters.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_transport::SecurityConfigBuilder;
    /// let validator = SecurityConfigBuilder::for_production()
    ///     .with_allowed_origins(vec!["https://app.example.com".to_string()])
    ///     .with_api_keys(vec!["secret-key".to_string()])
    ///     .build();
    /// ```
    pub fn for_production() -> Self {
        Self {
            origin_config: OriginConfig::for_production(Vec::new()),
            auth_config: AuthConfig::for_production(Vec::new(), AuthMethod::Bearer),
            rate_limit_config: Some(RateLimitConfig::for_production(100, Duration::from_secs(60))),
        }
    }

    /// Create builder for testing environment
    pub fn for_testing() -> Self {
        Self {
            origin_config: OriginConfig::for_testing(),
            auth_config: AuthConfig::for_testing(),
            rate_limit_config: Some(RateLimitConfig::for_testing()),
        }
    }

    /// Set allowed origins for CORS
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.origin_config.allowed_origins = origins.into_iter().collect();
        self
    }

    /// Add a single allowed origin
    pub fn add_allowed_origin(mut self, origin: String) -> Self {
        self.origin_config.allowed_origins.insert(origin);
        self
    }

    /// Allow localhost origins (localhost and 127.0.0.1)
    pub fn allow_localhost(mut self, allow: bool) -> Self {
        self.origin_config.allow_localhost = allow;
        self
    }

    /// Allow any origin (wildcard '*' - use with caution in production)
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.origin_config.allow_any = allow;
        self
    }

    /// Require authentication
    pub fn require_authentication(mut self, require: bool) -> Self {
        self.auth_config.require_auth = require;
        self
    }

    /// Set API keys for authentication
    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.auth_config.api_keys = keys.into_iter().collect();
        self
    }

    /// Add a single API key
    pub fn add_api_key(mut self, key: String) -> Self {
        self.auth_config.api_keys.insert(key);
        self
    }

    /// Set authentication method
    pub fn with_auth_method(mut self, method: AuthMethod) -> Self {
        self.auth_config.method = method;
        self
    }

    /// Set rate limiting parameters
    pub fn with_rate_limit(mut self, max_requests: usize, window: Duration) -> Self {
        self.rate_limit_config = Some(RateLimitConfig {
            max_requests,
            window,
            enabled: true,
        });
        self
    }

    /// Set rate limiting configuration
    pub fn with_rate_limit_config(mut self, config: RateLimitConfig) -> Self {
        self.rate_limit_config = Some(config);
        self
    }

    /// Disable rate limiting
    pub fn disable_rate_limiting(mut self) -> Self {
        self.rate_limit_config = None;
        self
    }

    /// Enable rate limiting with default settings
    pub fn enable_rate_limiting(mut self) -> Self {
        if self.rate_limit_config.is_none() {
            self.rate_limit_config = Some(RateLimitConfig::default());
        } else if let Some(ref mut config) = self.rate_limit_config {
            config.enabled = true;
        }
        self
    }

    /// Build the security validator
    pub fn build(self) -> SecurityValidator {
        SecurityValidator::new(self.origin_config, self.auth_config, self.rate_limit_config)
    }

    /// Get the current origin configuration
    pub fn origin_config(&self) -> &OriginConfig {
        &self.origin_config
    }

    /// Get the current auth configuration
    pub fn auth_config(&self) -> &AuthConfig {
        &self.auth_config
    }

    /// Get the current rate limit configuration
    pub fn rate_limit_config(&self) -> Option<&RateLimitConfig> {
        self.rate_limit_config.as_ref()
    }
}

/// Enhanced security configuration builder for session security
#[derive(Debug)]
pub struct EnhancedSecurityConfigBuilder {
    security_config: SecurityConfigBuilder,
    session_config: SessionSecurityConfig,
}

impl Default for EnhancedSecurityConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhancedSecurityConfigBuilder {
    /// Create a new enhanced security configuration builder
    pub fn new() -> Self {
        Self {
            security_config: SecurityConfigBuilder::new(),
            session_config: SessionSecurityConfig::default(),
        }
    }

    /// Create builder for development environment
    pub fn for_development() -> Self {
        Self {
            security_config: SecurityConfigBuilder::for_development(),
            session_config: SessionSecurityConfig::for_development(),
        }
    }

    /// Create builder for production environment with recommended defaults
    ///
    /// Returns a builder with production-grade presets for both security and sessions.
    /// Use builder methods to customize specific settings while keeping other defaults.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_transport::EnhancedSecurityConfigBuilder;
    /// let (validator, session_mgr) = EnhancedSecurityConfigBuilder::for_production()
    ///     .with_allowed_origins(vec!["https://app.example.com".to_string()])
    ///     .with_api_keys(vec!["secret-key".to_string()])
    ///     .build();
    /// ```
    pub fn for_production() -> Self {
        Self {
            security_config: SecurityConfigBuilder::for_production(),
            session_config: SessionSecurityConfig::for_production(),
        }
    }

    /// Create builder for testing environment
    pub fn for_testing() -> Self {
        Self {
            security_config: SecurityConfigBuilder::for_testing(),
            session_config: SessionSecurityConfig::for_testing(),
        }
    }

    // Basic security configuration methods (delegate to SecurityConfigBuilder)

    /// Set allowed origins for CORS
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.security_config = self.security_config.with_allowed_origins(origins);
        self
    }

    /// Add a single allowed origin
    pub fn add_allowed_origin(mut self, origin: String) -> Self {
        self.security_config = self.security_config.add_allowed_origin(origin);
        self
    }

    /// Allow localhost origins (localhost and 127.0.0.1)
    pub fn allow_localhost(mut self, allow: bool) -> Self {
        self.security_config = self.security_config.allow_localhost(allow);
        self
    }

    /// Allow any origin (wildcard '*' - use with caution in production)
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.security_config = self.security_config.allow_any_origin(allow);
        self
    }

    /// Require authentication
    pub fn require_authentication(mut self, require: bool) -> Self {
        self.security_config = self.security_config.require_authentication(require);
        self
    }

    /// Set API keys for authentication
    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.security_config = self.security_config.with_api_keys(keys);
        self
    }

    /// Add a single API key
    pub fn add_api_key(mut self, key: String) -> Self {
        self.security_config = self.security_config.add_api_key(key);
        self
    }

    /// Set authentication method
    pub fn with_auth_method(mut self, method: AuthMethod) -> Self {
        self.security_config = self.security_config.with_auth_method(method);
        self
    }

    /// Set rate limiting parameters
    pub fn with_rate_limit(mut self, max_requests: usize, window: Duration) -> Self {
        self.security_config = self.security_config.with_rate_limit(max_requests, window);
        self
    }

    /// Disable rate limiting
    pub fn disable_rate_limiting(mut self) -> Self {
        self.security_config = self.security_config.disable_rate_limiting();
        self
    }

    // Session security configuration methods

    /// Set session maximum lifetime
    pub fn with_session_max_lifetime(mut self, lifetime: Duration) -> Self {
        self.session_config.max_lifetime = lifetime;
        self
    }

    /// Set session idle timeout
    pub fn with_session_idle_timeout(mut self, timeout: Duration) -> Self {
        self.session_config.idle_timeout = timeout;
        self
    }

    /// Set maximum sessions per IP
    pub fn with_max_sessions_per_ip(mut self, max_sessions: usize) -> Self {
        self.session_config.max_sessions_per_ip = max_sessions;
        self
    }

    /// Enforce IP binding for sessions
    pub fn enforce_ip_binding(mut self, enforce: bool) -> Self {
        self.session_config.enforce_ip_binding = enforce;
        self
    }

    /// Enable session ID regeneration
    pub fn enable_session_id_regeneration(mut self, enable: bool, interval: Duration) -> Self {
        self.session_config.regenerate_session_ids = enable;
        self.session_config.regeneration_interval = interval;
        self
    }

    /// Set session security configuration
    pub fn with_session_config(mut self, config: SessionSecurityConfig) -> Self {
        self.session_config = config;
        self
    }

    /// Build enhanced security configuration
    pub fn build(self) -> (SecurityValidator, SessionSecurityManager) {
        let validator = self.security_config.build();
        let session_manager = SessionSecurityManager::new(self.session_config);
        (validator, session_manager)
    }

    /// Get the current security configuration builder
    pub fn security_config(&self) -> &SecurityConfigBuilder {
        &self.security_config
    }

    /// Get the current session configuration
    pub fn session_config(&self) -> &SessionSecurityConfig {
        &self.session_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_config_builder_default() {
        let builder = SecurityConfigBuilder::new();
        let validator = builder.build();

        assert!(validator.origin_config().allow_localhost);
        assert!(!validator.auth_config().require_auth);
        assert!(validator.rate_limiter().is_some());
    }

    #[test]
    fn test_security_config_builder_chaining() {
        let validator = SecurityConfigBuilder::new()
            .allow_localhost(false)
            .require_authentication(true)
            .with_api_keys(vec!["secret123".to_string()])
            .with_auth_method(AuthMethod::Bearer)
            .with_rate_limit(50, Duration::from_secs(30))
            .build();

        assert!(!validator.origin_config().allow_localhost);
        assert!(validator.auth_config().require_auth);
        assert!(validator.auth_config().api_keys.contains("secret123"));

        if let Some(limiter) = validator.rate_limiter() {
            assert_eq!(limiter.config().max_requests, 50);
            assert_eq!(limiter.config().window, Duration::from_secs(30));
        }
    }

    #[test]
    fn test_security_config_builder_for_development() {
        let validator = SecurityConfigBuilder::for_development().build();

        assert!(validator.origin_config().allow_localhost);
        assert!(!validator.auth_config().require_auth);

        // Rate limiting should be disabled for development
        if let Some(limiter) = validator.rate_limiter() {
            assert!(!limiter.config().enabled);
        }
    }

    #[test]
    fn test_security_config_builder_for_production() {
        let validator = SecurityConfigBuilder::for_production()
            .with_allowed_origins(vec!["https://app.example.com".to_string()])
            .with_api_keys(vec!["prod-key".to_string()])
            .build();

        assert!(!validator.origin_config().allow_localhost);
        assert!(validator.auth_config().require_auth);
        assert!(
            validator
                .origin_config()
                .allowed_origins
                .contains("https://app.example.com")
        );
        assert!(validator.auth_config().api_keys.contains("prod-key"));
    }

    #[test]
    fn test_security_config_builder_disable_rate_limiting() {
        let validator = SecurityConfigBuilder::new().disable_rate_limiting().build();

        assert!(validator.rate_limiter().is_none());
    }

    #[test]
    fn test_enhanced_security_config_builder() {
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .with_max_sessions_per_ip(5)
            .with_session_idle_timeout(Duration::from_secs(15 * 60))
            .enforce_ip_binding(true)
            .enable_session_id_regeneration(true, Duration::from_secs(30 * 60))
            .build();

        assert!(validator.origin_config().allow_localhost);
        assert_eq!(session_manager.config().max_sessions_per_ip, 5);
        assert_eq!(
            session_manager.config().idle_timeout,
            Duration::from_secs(15 * 60)
        );
        assert!(session_manager.config().enforce_ip_binding);
        assert!(session_manager.config().regenerate_session_ids);
    }

    #[test]
    fn test_enhanced_security_config_builder_for_environments() {
        // Development
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::for_development().build();
        assert!(validator.origin_config().allow_localhost);
        assert!(!session_manager.config().enforce_ip_binding);

        // Production
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::for_production()
            .with_allowed_origins(vec!["https://prod.com".to_string()])
            .with_api_keys(vec!["prod-key".to_string()])
            .build();
        assert!(!validator.origin_config().allow_localhost);
        assert!(session_manager.config().enforce_ip_binding);
        assert_eq!(session_manager.config().max_sessions_per_ip, 5);

        // Testing
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::for_testing().build();
        assert!(validator.origin_config().allow_any);
        assert_eq!(session_manager.config().max_sessions_per_ip, 2);
    }

    #[test]
    fn test_builder_add_methods() {
        let validator = SecurityConfigBuilder::new()
            .add_allowed_origin("https://app1.com".to_string())
            .add_allowed_origin("https://app2.com".to_string())
            .add_api_key("key1".to_string())
            .add_api_key("key2".to_string())
            .require_authentication(true)
            .build();

        assert!(
            validator
                .origin_config()
                .allowed_origins
                .contains("https://app1.com")
        );
        assert!(
            validator
                .origin_config()
                .allowed_origins
                .contains("https://app2.com")
        );
        assert!(validator.auth_config().api_keys.contains("key1"));
        assert!(validator.auth_config().api_keys.contains("key2"));
    }

    #[test]
    fn test_builder_inspection_methods() {
        let builder = SecurityConfigBuilder::new()
            .require_authentication(true)
            .with_api_keys(vec!["test-key".to_string()]);

        // Test inspection methods
        assert!(builder.auth_config().require_auth);
        assert!(builder.auth_config().api_keys.contains("test-key"));
        assert!(builder.rate_limit_config().is_some());
    }
}
