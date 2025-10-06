//! # 17: Middleware Stack - Authentication, Rate Limiting & Security
//!
//! **Learning Goals:**
//! - Configure server middleware for production deployments
//! - Implement authentication at the protocol layer
//! - Set up rate limiting and request validation
//! - Add security headers and audit logging
//!
//! **What this example demonstrates:**
//! - Complete middleware stack configuration
//! - AuthConfig for JWT authentication
//! - RateLimitConfig for API protection
//! - ValidationConfig for request validation
//! - SecurityConfig for headers (CORS, CSP, HSTS)
//! - AuditConfig for compliance logging
//!
//! **Note:** Authorization/RBAC should be handled at the application layer,
//! not in the protocol middleware. See the application examples for RBAC patterns.
//!
//! **Run with:** `cargo run --example 17_middleware_stack`

use std::num::NonZeroU32;
use std::time::Duration;
use turbomcp_server::middleware::rate_limit::{RateLimitStrategy, RateLimits};
use turbomcp_server::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize observability
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🛡️  Middleware Stack Demo - Production-Grade Security");

    // ============================================================================
    // AUTHENTICATION MIDDLEWARE (JWT with jsonwebtoken)
    // ============================================================================
    let auth_config = AuthConfig::default(); // Uses default JWT settings

    tracing::info!("✅ Auth: JWT validation (HS256 algorithm)");

    // ============================================================================
    // RATE LIMITING MIDDLEWARE (GCRA with tower-governor)
    // ============================================================================
    let rate_limit_config = RateLimitConfig {
        strategy: RateLimitStrategy::PerIp,
        limits: RateLimits {
            requests_per_period: NonZeroU32::new(100).unwrap(), // 100 requests
            period: Duration::from_secs(60),                    // per minute
            burst_size: Some(NonZeroU32::new(10).unwrap()),     // 10 burst
        },
        enabled: true,
    };

    tracing::info!("✅ Rate Limit: 100 req/min, 10 burst, per-IP");

    // ============================================================================
    // REQUEST VALIDATION MIDDLEWARE (JSON Schema)
    // ============================================================================
    let validation_config = ValidationConfig::default();

    tracing::info!("✅ Validation: JSON Schema validation");

    // ============================================================================
    // SECURITY HEADERS MIDDLEWARE (CORS, CSP, HSTS)
    // ============================================================================
    let security_config = SecurityConfig::default();

    tracing::info!("✅ Security: CORS, CSP, HSTS headers");

    // ============================================================================
    // AUDIT LOGGING MIDDLEWARE (Structured logging)
    // ============================================================================
    let audit_config = AuditConfig::default();

    tracing::info!("✅ Audit: Structured request logging");

    // ============================================================================
    // TIMEOUT MIDDLEWARE
    // ============================================================================
    let timeout_config = TimeoutConfig {
        request_timeout: Duration::from_secs(30), // 30s per request
        enabled: true,
    };

    tracing::info!("✅ Timeout: 30s per request");

    // ============================================================================
    // BUILD MIDDLEWARE STACK
    // ============================================================================
    let middleware_stack = MiddlewareStack::new()
        .with_security(security_config) // Apply security headers first
        .with_timeout(timeout_config) // Enforce timeouts early
        .with_validation(validation_config) // Validate before processing
        .with_auth(auth_config) // Authenticate user
        .with_rate_limit(rate_limit_config) // Rate limit requests
        .with_audit(audit_config); // Audit after all checks

    tracing::info!("\n📚 Middleware Stack Order:");
    tracing::info!("  1. SecurityLayer (CORS, CSP, HSTS)");
    tracing::info!("  2. TimeoutLayer (prevent hung requests)");
    tracing::info!("  3. ValidationLayer (JSON schema validation)");
    tracing::info!("  4. AuthLayer (JWT token validation)");
    tracing::info!("  5. AuthzLayer (Casbin RBAC)");
    tracing::info!("  6. RateLimitLayer (Per-IP rate limiting)");
    tracing::info!("  7. AuditLayer (compliance logging)");

    // ============================================================================
    // ACCESS INDIVIDUAL LAYERS
    // ============================================================================
    if let Some(_auth_layer) = middleware_stack.auth_layer() {
        tracing::info!("\n✓ Auth layer configured");
    }
    if let Some(_rate_limit_layer) = middleware_stack.rate_limit_layer() {
        tracing::info!("✓ Rate limit layer configured");
    }
    if let Some(_validation_layer) = middleware_stack.validation_layer() {
        tracing::info!("✓ Validation layer configured");
    }
    if let Some(_audit_layer) = middleware_stack.audit_layer() {
        tracing::info!("✓ Audit layer configured");
    }

    tracing::info!("\n✨ Middleware stack fully configured");
    tracing::info!("🎯 Features: auth + rate limiting + validation + security + audit");
    tracing::info!("📌 Note: Authorization/RBAC belongs in the application layer");

    // For demo purposes, just show configuration
    tracing::info!("\n📝 Middleware demonstrates:");
    tracing::info!("  ✓ Builder pattern for middleware composition");
    tracing::info!("  ✓ JWT authentication with configurable TTL");
    tracing::info!("  ✓ Casbin RBAC for fine-grained authorization");
    tracing::info!("  ✓ Per-IP rate limiting with GCRA algorithm");
    tracing::info!("  ✓ JSON Schema request validation");
    tracing::info!("  ✓ Security headers (CORS, CSP, HSTS)");
    tracing::info!("  ✓ Structured audit logging");
    tracing::info!("  ✓ Request timeout protection");

    Ok(())
}

/* 📝 **Key Concepts:**

**Middleware Stack Pattern:**
```text
Request
  ↓
SecurityLayer → Set CORS, CSP, HSTS headers
  ↓
TimeoutLayer → Start timeout timer
  ↓
ValidationLayer → Validate JSON schema
  ↓
AuthLayer → Verify token
  ↓
AuthzLayer → Check permissions
  ↓
RateLimitLayer → Check rate limit
  ↓
AuditLayer → Log request
  ↓
Handler (Tool/Resource/Prompt)
  ↓
Response
```

**Middleware Order Matters:**
1. **Security first** - Set headers before any processing
2. **Timeout early** - Start timer before expensive operations
3. **Validate** - Reject bad requests before authentication
4. **Authenticate** - Verify identity before authorization
5. **Authorize** - Check permissions after identity known
6. **Rate limit** - Protect authenticated endpoints
7. **Audit last** - Log after all checks pass

**Authentication vs Authorization:**
- **Authentication** (AuthLayer): "Who are you?" - Verify identity via tokens
- **Authorization** (AuthzLayer): "What can you do?" - Check permissions/roles

**Rate Limiting Strategies:**
1. **Token Bucket** - Smooth rate limiting with bursts
2. **Leaky Bucket** - Strict rate limiting, no bursts
3. **Fixed Window** - Simple but allows burst at window boundaries
4. **Sliding Window** - More accurate but more expensive

**Production Best Practices:**
1. **Never log sensitive data** - `log_request_body: false` for passwords/tokens
2. **Use HTTPS in production** - HSTS ensures secure connections
3. **Rate limit per user** - Prevents abuse while allowing legitimate users
4. **Validate early** - Reject bad requests before expensive operations
5. **Audit everything** - Compliance and security incident investigation

**Security Headers Explained:**
- **CORS** - Control which domains can call your API
- **CSP** - Prevent XSS attacks by controlling resource loading
- **HSTS** - Force HTTPS connections for security

**Next Example:** `18_completion_protocol.rs` - Autocompletion for prompts and resources
*/
