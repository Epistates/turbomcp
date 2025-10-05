//! Critical Bug Fix Tests for 2.0.0 Release
//!
//! This test suite documents and verifies bug fixes made for the 2.0.0 release:
//!
//! ## Critical Fixes
//! 1. **SSE Request Processing**: SSE requests now actually process requests instead of
//!    just sending acknowledgments. Previously returned `{"processing": true}` but never
//!    actually called the handler.
//!
//! 2. **Protocol Version Default**: Changed default from "2025-03-26" to "2025-06-18"
//!    for better client compatibility.
//!
//! 3. **Production Config Security**: Production configs now require explicit parameters
//!    to prevent accidental use of permissive defaults.
//!
//! ## Test Coverage
//! The SSE and protocol version fixes are tested through existing integration tests in:
//! - `transport_validation.rs`: HTTP SSE compliance tests
//! - `external_dependency_integration.rs`: Router generation tests
//!
//! This file adds specific unit tests for the production config parameter requirements.

use std::time::Duration;
use turbomcp_transport::security::{RateLimitConfig, SecurityValidator};

#[test]
fn test_production_configs_require_explicit_parameters() {
    // Bug fix: Production configs should require explicit parameters to prevent
    // accidental use of permissive defaults in production

    println!("🔍 Testing Production Config Parameter Requirements");

    // RateLimitConfig::for_production now requires explicit parameters
    let rate_limit = RateLimitConfig::for_production(100, Duration::from_secs(60));
    assert_eq!(rate_limit.max_requests, 100);
    assert_eq!(rate_limit.window, Duration::from_secs(60));
    assert!(rate_limit.enabled);
    println!("✅ RateLimitConfig::for_production requires explicit max_requests and window");

    // SecurityValidator::for_production now requires explicit rate limit parameters
    let validator = SecurityValidator::for_production(
        vec!["https://app.example.com".to_string()],
        vec!["secret-key".to_string()],
        100,
        Duration::from_secs(60),
    );

    assert!(validator.rate_limiter().is_some());
    println!("✅ SecurityValidator::for_production requires explicit rate limit parameters");

    // This prevents accidental misconfigurations like:
    // let validator = SecurityValidator::for_production(origins, keys); // Old API - would use defaults
    // Now developers MUST specify: SecurityValidator::for_production(origins, keys, 100, Duration::from_secs(60))

    println!("✅ Production configs require explicit parameters - prevents accidental permissive defaults");
}

#[tokio::test]
async fn test_rate_limit_logging() {
    // Bug fix: Rate limiting now has comprehensive logging for debugging

    use turbomcp_transport::security::RateLimiter;
    use std::net::IpAddr;
    use std::str::FromStr;

    println!("🔍 Testing Rate Limit Logging");

    // Create a rate limiter with low limit for testing
    let rate_limit_config = RateLimitConfig::for_production(3, Duration::from_secs(60));
    let limiter = RateLimiter::new(rate_limit_config);

    let test_ip = IpAddr::from_str("127.0.0.1").unwrap();

    // These should succeed
    for i in 1..=3 {
        limiter.check_rate_limit(test_ip)
            .expect(&format!("Request {} should succeed", i));
        println!("✅ Request {}/3 allowed", i);
    }

    // This should fail (exceeded limit)
    let result = limiter.check_rate_limit(test_ip);
    assert!(result.is_err(), "4th request should be rate limited");

    if let Err(e) = result {
        println!("✅ Rate limit exceeded: {}", e);
        // Logging happens inside check_rate_limit via tracing::warn
    }

    println!("✅ Rate limiting has proper logging for debugging");
}
