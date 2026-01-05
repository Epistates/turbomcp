//! Comprehensive middleware stack for MCP server
//!
//! This module provides a comprehensive middleware stack using well-established libraries
//! for security, validation, and cross-cutting concerns. Each middleware is focused
//! on a single responsibility and can be composed to create secure, robust MCP servers.
//!
//! # Architecture
//!
//! The middleware stack follows the Tower pattern and is ordered for optimal security:
//! 1. Security headers and CORS
//! 2. Authentication (JWT verification)
//! 3. Rate limiting (tower-governor)
//! 4. Request validation (JSON Schema)
//! 5. Audit logging
//! 6. Timeout management
//! 7. Business handlers (pure logic)
//!
//! **Note**: Authorization/RBAC should be handled at the application layer, not in the
//! protocol middleware. The MCP server focuses on protocol-level concerns.

pub mod audit;
#[cfg(feature = "auth")]
pub mod auth;
#[cfg(feature = "input-validation")]
pub mod input_validation;
#[cfg(feature = "rate-limiting")]
pub mod rate_limit;
pub mod security;
#[cfg(feature = "multi-tenancy")]
pub mod tenancy;
pub mod timeout;
pub mod validation;

pub use audit::{AuditConfig, AuditLayer};
#[cfg(feature = "auth")]
#[allow(deprecated)]
pub use auth::{AuthConfig, AuthContext, AuthLayer, Claims, UserInfo};
#[cfg(feature = "input-validation")]
pub use input_validation::{Garde, Validate, patterns, validators};
#[cfg(feature = "rate-limiting")]
pub use rate_limit::{RateLimitConfig, RateLimitLayer, mcp_rate_limit_error_handler};
pub use security::{SecurityConfig, SecurityLayer};
#[cfg(feature = "multi-tenancy")]
pub use tenancy::{
    ApiKeyTenantExtractor, CompositeTenantExtractor, HeaderTenantExtractor,
    SubdomainTenantExtractor, TenantExtractionLayer, TenantExtractionService, TenantExtractor,
    TenantId,
};
pub use timeout::{TimeoutConfig, TimeoutLayer};
pub use validation::{ValidationConfig, ValidationLayer};

use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

/// Complete middleware stack builder for MCP servers
#[derive(Debug, Clone)]
pub struct MiddlewareStack {
    #[cfg(feature = "auth")]
    pub(crate) auth_config: Option<AuthConfig>,
    #[cfg(feature = "rate-limiting")]
    pub(crate) rate_limit_config: Option<RateLimitConfig>,
    pub(crate) validation_config: Option<ValidationConfig>,
    pub(crate) security_config: SecurityConfig,
    pub(crate) audit_config: Option<AuditConfig>,
    pub(crate) timeout_config: Option<TimeoutConfig>,
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self {
            #[cfg(feature = "auth")]
            auth_config: None,
            #[cfg(feature = "rate-limiting")]
            rate_limit_config: None,
            validation_config: Some(ValidationConfig::default()),
            security_config: SecurityConfig::default(),
            audit_config: None,
            timeout_config: Some(TimeoutConfig::default()),
        }
    }
}

impl MiddlewareStack {
    /// Create a new middleware stack builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable JWT authentication
    #[cfg(feature = "auth")]
    pub fn with_auth(mut self, config: AuthConfig) -> Self {
        self.auth_config = Some(config);
        self
    }

    /// Enable rate limiting
    #[cfg(feature = "rate-limiting")]
    pub fn with_rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.rate_limit_config = Some(config);
        self
    }

    /// Configure request validation
    pub fn with_validation(mut self, config: ValidationConfig) -> Self {
        self.validation_config = Some(config);
        self
    }

    /// Configure security headers and CORS
    pub fn with_security(mut self, config: SecurityConfig) -> Self {
        self.security_config = config;
        self
    }

    /// Enable audit logging
    pub fn with_audit(mut self, config: AuditConfig) -> Self {
        self.audit_config = Some(config);
        self
    }

    /// Configure request timeouts
    pub fn with_timeout(mut self, config: TimeoutConfig) -> Self {
        self.timeout_config = Some(config);
        self
    }

    /// Build the basic middleware stack (security, tracing, compression, timeout)
    ///
    /// This creates a production-ready base stack with:
    /// 1. Security headers and CORS
    /// 2. Request ID and distributed tracing
    /// 3. Response compression
    /// 4. Request timeout (always applied for DoS protection)
    ///
    /// For advanced middleware (auth, rate limiting, validation, audit),
    /// use the individual layer builders and compose manually, or use preset stacks.
    pub fn build<S>(self) -> impl tower::Layer<S>
    where
        S: Clone + Send + 'static,
    {
        // Use configured timeout or default to 30 seconds
        let timeout = self.timeout_config.unwrap_or_default();

        ServiceBuilder::new()
            // 1. Security headers and CORS (outermost layer)
            .layer(SecurityLayer::new(self.security_config).build())
            // 2. Request ID and tracing
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(TraceLayer::new_for_http())
            // 3. Request timeout (DoS protection)
            .layer(tower_http::timeout::TimeoutLayer::with_status_code(
                http::StatusCode::REQUEST_TIMEOUT,
                timeout.request_timeout,
            ))
            // 4. Response compression
            .layer(CompressionLayer::new())
            .into_inner()
    }

    /// Get the auth layer if configured
    #[cfg(feature = "auth")]
    pub fn auth_layer(&self) -> Option<AuthLayer> {
        self.auth_config.clone().map(AuthLayer::new)
    }

    /// Get the audit layer if configured
    pub fn audit_layer(&self) -> Option<AuditLayer> {
        self.audit_config.clone().map(AuditLayer::new)
    }

    /// Get the validation layer if configured
    pub fn validation_layer(&self) -> Option<ValidationLayer> {
        self.validation_config.clone().map(ValidationLayer::new)
    }

    /// Get the rate limit layer if configured
    #[cfg(feature = "rate-limiting")]
    pub fn rate_limit_layer(&self) -> Option<RateLimitLayer> {
        self.rate_limit_config.clone().map(RateLimitLayer::new)
    }
}

// Comprehensive tests in separate file (tokio/axum pattern)
#[cfg(test)]
mod tests;
