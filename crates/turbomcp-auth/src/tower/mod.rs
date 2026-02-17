//! # Tower Middleware Integration for TurboMCP Auth
//!
//! This module provides Tower Layer and Service implementations for authentication
//! and rate limiting, enabling composable middleware stacks that integrate with the
//! Tower ecosystem.
//!
//! ## Overview
//!
//! The Tower integration consists of:
//!
//! - [`AuthLayer`] - A Tower Layer that wraps services with authentication
//! - [`AuthService`] - A Tower Service that performs token extraction and validation
//! - [`RateLimitLayer`] - A Tower Layer that adds rate limiting
//! - [`RateLimitService`] - A Tower Service that enforces rate limits
//!
//! ## Usage
//!
//! ### Authentication Only
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_auth::tower::{AuthLayer, AuthConfig};
//! use turbomcp_auth::AuthProvider;
//!
//! let auth_layer = AuthLayer::new(auth_provider);
//!
//! let service = ServiceBuilder::new()
//!     .layer(auth_layer)
//!     .service(my_inner_service);
//! ```
//!
//! ### Rate Limiting + Authentication
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_auth::tower::{AuthLayer, RateLimitLayer};
//! use turbomcp_auth::rate_limit::RateLimiter;
//!
//! let service = ServiceBuilder::new()
//!     .layer(RateLimitLayer::new(RateLimiter::for_auth()))
//!     .layer(AuthLayer::new(auth_provider))
//!     .service(my_inner_service);
//! ```
//!
//! ## Request Extensions
//!
//! On successful authentication, the `AuthContext` is inserted into the request's
//! extensions, making it available to inner services:
//!
//! ```rust,ignore
//! // In your inner service handler
//! if let Some(auth_ctx) = req.extensions().get::<AuthContext>() {
//!     println!("Authenticated user: {}", auth_ctx.sub);
//! }
//! ```

mod layer;
pub mod rate_limit;
mod service;

pub use layer::AuthLayer;
pub use rate_limit::{
    IpKeyExtractor, KeyExtractor, RateLimitLayer, RateLimitRejection, RateLimitService,
};
pub use service::{AuthService, AuthServiceFuture};

/// Configuration for the auth layer
#[derive(Debug, Clone)]
pub struct AuthLayerConfig {
    /// Whether to allow unauthenticated requests to pass through
    pub allow_anonymous: bool,
    /// Methods that bypass authentication (e.g., "initialize", "ping")
    pub bypass_methods: Vec<String>,
    /// Header name to extract token from (default: "Authorization")
    pub auth_header: String,
    /// Alternative header for API keys (default: "X-API-Key")
    pub api_key_header: String,
}

impl Default for AuthLayerConfig {
    fn default() -> Self {
        Self {
            allow_anonymous: false,
            bypass_methods: vec!["initialize".to_string(), "ping".to_string()],
            auth_header: "Authorization".to_string(),
            api_key_header: "X-API-Key".to_string(),
        }
    }
}

impl AuthLayerConfig {
    /// Create a new config that allows anonymous access
    #[must_use]
    pub fn allow_anonymous() -> Self {
        Self {
            allow_anonymous: true,
            ..Default::default()
        }
    }

    /// Create a new config with custom bypass methods
    #[must_use]
    pub fn with_bypass_methods(methods: Vec<String>) -> Self {
        Self {
            bypass_methods: methods,
            ..Default::default()
        }
    }

    /// Add a method to the bypass list
    #[must_use]
    pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
        self.bypass_methods.push(method.into());
        self
    }

    /// Set the authorization header name
    #[must_use]
    pub fn auth_header(mut self, header: impl Into<String>) -> Self {
        self.auth_header = header.into();
        self
    }

    /// Set the API key header name
    #[must_use]
    pub fn api_key_header(mut self, header: impl Into<String>) -> Self {
        self.api_key_header = header.into();
        self
    }

    /// Check if a method should bypass authentication
    #[must_use]
    pub fn should_bypass(&self, method: &str) -> bool {
        self.bypass_methods.iter().any(|m| m == method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AuthLayerConfig::default();
        assert!(!config.allow_anonymous);
        assert!(config.bypass_methods.contains(&"initialize".to_string()));
        assert!(config.bypass_methods.contains(&"ping".to_string()));
        assert_eq!(config.auth_header, "Authorization");
        assert_eq!(config.api_key_header, "X-API-Key");
    }

    #[test]
    fn test_allow_anonymous() {
        let config = AuthLayerConfig::allow_anonymous();
        assert!(config.allow_anonymous);
    }

    #[test]
    fn test_should_bypass() {
        let config = AuthLayerConfig::default();
        assert!(config.should_bypass("initialize"));
        assert!(config.should_bypass("ping"));
        assert!(!config.should_bypass("tools/call"));
    }

    #[test]
    fn test_custom_bypass_methods() {
        let config =
            AuthLayerConfig::with_bypass_methods(vec!["health".to_string()]).bypass_method("ready");
        assert!(config.should_bypass("health"));
        assert!(config.should_bypass("ready"));
        assert!(!config.should_bypass("initialize")); // Not in custom list
    }

    #[test]
    fn test_custom_headers() {
        let config = AuthLayerConfig::default()
            .auth_header("X-Auth-Token")
            .api_key_header("X-Custom-Key");
        assert_eq!(config.auth_header, "X-Auth-Token");
        assert_eq!(config.api_key_header, "X-Custom-Key");
    }
}
