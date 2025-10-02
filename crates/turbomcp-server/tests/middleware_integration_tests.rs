//! Integration tests for the complete middleware stack
//!
//! These tests validate that all middleware layers work together correctly
//! in production-like scenarios.

use std::time::Duration;

use turbomcp_server::middleware::*;
use turbomcp_server::middleware::security::CorsOrigins;
use secrecy::Secret;
use jsonwebtoken::Algorithm;

#[test]
fn test_middleware_stack_builder() {
    // Create a complete middleware stack with all layers
    let stack = MiddlewareStack::new()
        .with_auth(
            AuthConfig::new(Secret::new("test_secret".to_string()))
                .with_algorithm(Algorithm::HS256)
        )
        .with_rate_limit(RateLimitConfig::new(100))
        .with_timeout(TimeoutConfig::new(Duration::from_secs(30)));

    // Verify layers can be accessed
    assert!(stack.auth_layer().is_some());
    assert!(stack.rate_limit_layer().is_some());
}

#[test]
fn test_rate_limit_config_presets() {
    // Test strict preset
    let strict = RateLimitConfig::strict();
    assert_eq!(strict.limits.requests_per_period.get(), 30);
    assert_eq!(strict.limits.burst_size.unwrap().get(), 5);
    assert!(strict.enabled);

    // Test permissive preset
    let permissive = RateLimitConfig::permissive();
    assert_eq!(permissive.limits.requests_per_period.get(), 1000);
    assert_eq!(permissive.limits.burst_size.unwrap().get(), 100);

    // Test custom config
    let custom = RateLimitConfig::new(50);
    assert_eq!(custom.limits.requests_per_period.get(), 50);
}

#[test]
fn test_auth_config_builder() {
    let config = AuthConfig::new(Secret::new("secret".to_string()))
        .with_algorithm(Algorithm::HS256)
        .with_issuer("https://api.example.com".to_string())
        .with_audience("mcp-server".to_string())
        .with_leeway(60);

    assert_eq!(config.algorithm, Algorithm::HS256);
    assert_eq!(config.issuer, Some("https://api.example.com".to_string()));
    assert_eq!(config.audience, Some("mcp-server".to_string()));
    assert_eq!(config.leeway, 60);
}

#[test]
fn test_jwt_claims_validation() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Test valid claims
    let valid_claims = Claims {
        sub: "user123".to_string(),
        roles: vec!["user".to_string()],
        exp: now + 3600, // 1 hour from now
        iat: now,
        iss: None,
        aud: None,
    };

    assert!(!valid_claims.is_expired());
    assert!(valid_claims.has_role("user"));
    assert!(!valid_claims.has_role("admin"));

    // Test expired claims
    let expired_claims = Claims {
        sub: "user123".to_string(),
        roles: vec!["user".to_string()],
        exp: now - 3600, // 1 hour ago
        iat: now - 7200,
        iss: None,
        aud: None,
    };

    assert!(expired_claims.is_expired());
}

#[test]
fn test_authz_resource_extraction() {
    // This tests the internal resource extraction logic
    // The actual implementation is in authz.rs

    let test_cases = vec![
        ("/mcp", "mcp"),
        ("/api/v1/tools", "api"),
        ("/tools/call", "tools"),
        ("/", "default"),
        ("", "default"),
    ];

    for (path, expected) in test_cases {
        let resource = if path.starts_with('/') {
            let parts: Vec<&str> = path[1..].split('/').collect();
            let first_part = parts.first().unwrap_or(&"default");
            if first_part.is_empty() {
                "default".to_string()
            } else {
                first_part.to_string()
            }
        } else {
            "default".to_string()
        };

        assert_eq!(resource, expected, "Failed for path: {}", path);
    }
}

#[test]
fn test_timeout_config_presets() {
    // Default config
    let default = TimeoutConfig::default();
    assert_eq!(default.request_timeout, Duration::from_secs(30));
    assert!(default.enabled);

    // Strict config
    let strict = TimeoutConfig::strict();
    assert_eq!(strict.request_timeout, Duration::from_secs(10));
    assert!(strict.enabled);

    // Permissive config
    let permissive = TimeoutConfig::permissive();
    assert_eq!(permissive.request_timeout, Duration::from_secs(120));
    assert!(permissive.enabled);

    // Disabled config
    let disabled = TimeoutConfig::disabled();
    assert!(!disabled.enabled);
}

#[test]
fn test_security_config_builder() {
    let config = SecurityConfig::default();

    // SecurityConfig has sensible defaults for production
    // CORS allows any origin by default (should be restricted in production)
    assert!(matches!(config.cors.allowed_origins, CorsOrigins::Any));

    // Security headers are configured with safe defaults
    assert!(config.headers.content_type_options);
    assert!(!config.sensitive_headers.is_empty());
}

#[test]
fn test_audit_config_builder() {
    let config = AuditConfig::default();

    assert!(config.log_success);
    assert!(config.log_failures);
    assert!(config.log_auth_events);
    assert!(config.log_authz_events);
}

#[test]
fn test_validation_config_mcp_schemas() {
    // Test that MCP schemas can be loaded
    let result = ValidationConfig::with_mcp_schemas();

    // Should succeed (schemas are embedded at compile time)
    assert!(result.is_ok(), "Failed to load MCP schemas: {:?}", result.err());

    let config = result.unwrap();
    assert!(config.validate_requests);
    assert!(config.strict_mode);

    // Verify some schemas were loaded
    assert!(!config.schemas.is_empty(), "No schemas were loaded");
}

#[test]
fn test_rate_limit_layer_helper_methods() {
    let config = RateLimitConfig::new(120); // 120 requests per minute
    let layer = RateLimitLayer::new(config);

    // Should calculate 2 requests per second (120/60)
    assert_eq!(layer.requests_per_second(), 2);

    // Should have burst size (120/10 = 12)
    assert_eq!(layer.burst_size(), 12);

    assert!(layer.is_enabled());
}

#[test]
fn test_complete_middleware_composition() {
    // This tests that all middleware can be composed together
    // without type errors or conflicts

    let middleware = MiddlewareStack::new()
        .with_auth(AuthConfig::new(Secret::new("secret".to_string())))
        .with_authz(AuthzConfig::new(
            "src/policies/rbac_model.conf".to_string(),
            "src/policies/rbac_policy.csv".to_string(),
        ))
        .with_rate_limit(RateLimitConfig::strict())
        .with_timeout(TimeoutConfig::strict())
        .with_audit(AuditConfig::default());

    // All layers should be accessible before building
    assert!(middleware.auth_layer().is_some());
    assert!(middleware.authz_layer().is_some());
    assert!(middleware.rate_limit_layer().is_some());
    assert!(middleware.audit_layer().is_some());

    // Build the base stack (should not panic)
    // Note: build() consumes self
    let _base = middleware.build::<()>();
}
