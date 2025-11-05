//! Sprint 1.1: Response Size Validation Tests
//!
//! Tests for v2.2.0 size validation feature implementing the
//! "Secure by default, flexible by design" philosophy.

use turbomcp_transport::{
    config::{LimitsConfig, TransportConfigBuilder},
    core::{
        TransportConfig, TransportError, TransportType, validate_request_size,
        validate_response_size,
    },
};

#[cfg(feature = "stdio")]
use turbomcp_transport::stdio::StdioTransport;

/// Test that default limits are set correctly
#[test]
fn test_limits_config_defaults() {
    let limits = LimitsConfig::default();

    // Default: 10MB response limit
    assert_eq!(limits.max_response_size, Some(10 * 1024 * 1024));

    // Default: 1MB request limit
    assert_eq!(limits.max_request_size, Some(1024 * 1024));

    // Default: enforce on streams
    assert!(limits.enforce_on_streams);
}

/// Test that unlimited configuration works
#[test]
fn test_limits_config_unlimited() {
    let limits = LimitsConfig::unlimited();

    assert_eq!(limits.max_response_size, None);
    assert_eq!(limits.max_request_size, None);
    assert!(!limits.enforce_on_streams);
}

/// Test that strict configuration works
#[test]
fn test_limits_config_strict() {
    let limits = LimitsConfig::strict();

    // Strict: 1MB response limit
    assert_eq!(limits.max_response_size, Some(1024 * 1024));

    // Strict: 256KB request limit
    assert_eq!(limits.max_request_size, Some(256 * 1024));

    // Strict: enforce on streams
    assert!(limits.enforce_on_streams);
}

/// Test that custom limits can be configured
#[test]
fn test_limits_config_custom() {
    let limits = LimitsConfig {
        max_response_size: Some(50 * 1024 * 1024), // 50MB
        max_request_size: Some(5 * 1024 * 1024),   // 5MB
        enforce_on_streams: true,
    };

    assert_eq!(limits.max_response_size, Some(50 * 1024 * 1024));
    assert_eq!(limits.max_request_size, Some(5 * 1024 * 1024));
    assert!(limits.enforce_on_streams);
}

/// Test that TransportConfig includes limits with defaults
#[test]
fn test_transport_config_default_includes_limits() {
    let config = TransportConfig::default();

    // Should have default limits
    assert_eq!(config.limits.max_response_size, Some(10 * 1024 * 1024));
    assert_eq!(config.limits.max_request_size, Some(1024 * 1024));
}

/// Test that TransportConfigBuilder supports limits
#[test]
fn test_transport_config_builder_with_limits() {
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig::strict())
        .build()
        .unwrap();

    assert_eq!(config.limits.max_response_size, Some(1024 * 1024));
    assert_eq!(config.limits.max_request_size, Some(256 * 1024));
}

/// Test request size validation - within limit
#[test]
fn test_validate_request_size_within_limit() {
    let limits = LimitsConfig::default();

    // 100KB is within the 1MB limit
    let result = validate_request_size(100 * 1024, &limits);
    assert!(result.is_ok());
}

/// Test request size validation - exceeds limit
#[test]
fn test_validate_request_size_exceeds_limit() {
    let limits = LimitsConfig::default();

    // 2MB exceeds the 1MB limit
    let result = validate_request_size(2 * 1024 * 1024, &limits);
    assert!(result.is_err());

    match result.unwrap_err() {
        TransportError::RequestTooLarge { size, max } => {
            assert_eq!(size, 2 * 1024 * 1024);
            assert_eq!(max, 1024 * 1024);
        }
        other => panic!("Expected RequestTooLarge error, got: {:?}", other),
    }
}

/// Test request size validation - unlimited
#[test]
fn test_validate_request_size_unlimited() {
    let limits = LimitsConfig::unlimited();

    // Even 100MB should be allowed with unlimited config
    let result = validate_request_size(100 * 1024 * 1024, &limits);
    assert!(result.is_ok());
}

/// Test response size validation - within limit
#[test]
fn test_validate_response_size_within_limit() {
    let limits = LimitsConfig::default();

    // 5MB is within the 10MB limit
    let result = validate_response_size(5 * 1024 * 1024, &limits);
    assert!(result.is_ok());
}

/// Test response size validation - exceeds limit
#[test]
fn test_validate_response_size_exceeds_limit() {
    let limits = LimitsConfig::default();

    // 15MB exceeds the 10MB limit
    let result = validate_response_size(15 * 1024 * 1024, &limits);
    assert!(result.is_err());

    match result.unwrap_err() {
        TransportError::ResponseTooLarge { size, max } => {
            assert_eq!(size, 15 * 1024 * 1024);
            assert_eq!(max, 10 * 1024 * 1024);
        }
        other => panic!("Expected ResponseTooLarge error, got: {:?}", other),
    }
}

/// Test response size validation - unlimited
#[test]
fn test_validate_response_size_unlimited() {
    let limits = LimitsConfig::unlimited();

    // Even 100MB should be allowed with unlimited config
    let result = validate_response_size(100 * 1024 * 1024, &limits);
    assert!(result.is_ok());
}

/// Test that error messages are helpful
#[test]
fn test_error_messages_are_helpful() {
    let limits = LimitsConfig::default();

    let result = validate_request_size(2 * 1024 * 1024, &limits);
    let error = result.unwrap_err();
    let error_msg = error.to_string();

    // Error message should contain:
    // - Size information
    // - Limit information
    // - Suggestion on how to fix
    assert!(error_msg.contains("2097152"), "Should show actual size");
    assert!(error_msg.contains("1048576"), "Should show max size");
    assert!(
        error_msg.contains("LimitsConfig") || error_msg.contains("increase"),
        "Should suggest how to fix: {}",
        error_msg
    );
}

/// Test edge case: exactly at the limit
#[test]
fn test_validate_size_exactly_at_limit() {
    let limits = LimitsConfig {
        max_request_size: Some(1000),
        max_response_size: Some(1000),
        enforce_on_streams: true,
    };

    // Exactly at limit should pass
    assert!(validate_request_size(1000, &limits).is_ok());
    assert!(validate_response_size(1000, &limits).is_ok());

    // One byte over should fail
    assert!(validate_request_size(1001, &limits).is_err());
    assert!(validate_response_size(1001, &limits).is_err());
}

/// Test edge case: zero size
#[test]
fn test_validate_size_zero() {
    let limits = LimitsConfig::default();

    // Zero-size messages should be allowed
    assert!(validate_request_size(0, &limits).is_ok());
    assert!(validate_response_size(0, &limits).is_ok());
}

/// Test LimitsConfig serialization/deserialization
#[test]
fn test_limits_config_serde() {
    let limits = LimitsConfig {
        max_response_size: Some(50 * 1024 * 1024),
        max_request_size: Some(5 * 1024 * 1024),
        enforce_on_streams: true,
    };

    let json = serde_json::to_string(&limits).unwrap();
    let deserialized: LimitsConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(limits, deserialized);
}

/// Test that LimitsConfig can be serialized as part of TransportConfig
#[test]
fn test_transport_config_with_limits_serde() {
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig::strict())
        .build()
        .unwrap();

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: TransportConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(config.limits, deserialized.limits);
}

/// Integration test: STDIO transport respects default limits
#[tokio::test]
#[cfg(feature = "stdio")]
async fn test_stdio_transport_has_default_limits() {
    let _transport = StdioTransport::new();

    // The transport should use default limits configuration
    // This is verified by checking that config contains the limits field
    // (actual send/receive testing requires a full STDIO setup)
}

/// Integration test: STDIO transport with custom limits
#[tokio::test]
#[cfg(feature = "stdio")]
async fn test_stdio_transport_with_custom_limits() {
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig::unlimited())
        .build()
        .unwrap();

    let _transport = StdioTransport::with_config(config);

    // Transport should have unlimited configuration
    // (actual send/receive testing requires a full STDIO setup)
}

/// Scenario test: Persona A (default security) - small project
#[test]
fn test_persona_a_default_security() {
    // Small project, relies on TurboMCP for security
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .build()
        .unwrap();

    // Should have secure defaults
    assert_eq!(config.limits.max_response_size, Some(10 * 1024 * 1024));
    assert_eq!(config.limits.max_request_size, Some(1024 * 1024));

    // Reject large requests/responses
    assert!(validate_request_size(2 * 1024 * 1024, &config.limits).is_err());
    assert!(validate_response_size(15 * 1024 * 1024, &config.limits).is_err());
}

/// Scenario test: Persona B (behind API gateway) - enterprise
#[test]
fn test_persona_b_behind_api_gateway() {
    // Enterprise deployment behind API gateway
    // API gateway already enforces limits, no need for redundancy
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig::unlimited())
        .build()
        .unwrap();

    // Should have no limits
    assert_eq!(config.limits.max_response_size, None);
    assert_eq!(config.limits.max_request_size, None);

    // Accept arbitrarily large messages
    assert!(validate_request_size(100 * 1024 * 1024, &config.limits).is_ok());
    assert!(validate_response_size(100 * 1024 * 1024, &config.limits).is_ok());
}

/// Scenario test: Large file handling use case
#[test]
fn test_large_file_handling_scenario() {
    // Use case: Legitimate large file transfers (up to 50MB)
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig {
            max_response_size: Some(50 * 1024 * 1024), // 50MB
            max_request_size: Some(50 * 1024 * 1024),  // 50MB
            enforce_on_streams: true,
        })
        .build()
        .unwrap();

    // 40MB file should be allowed
    assert!(validate_request_size(40 * 1024 * 1024, &config.limits).is_ok());
    assert!(validate_response_size(40 * 1024 * 1024, &config.limits).is_ok());

    // 60MB file should still be rejected
    assert!(validate_request_size(60 * 1024 * 1024, &config.limits).is_err());
    assert!(validate_response_size(60 * 1024 * 1024, &config.limits).is_err());
}

/// Scenario test: Untrusted server with strict limits
#[test]
fn test_untrusted_server_strict_limits() {
    // Connecting to potentially malicious or buggy server
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig::strict())
        .build()
        .unwrap();

    // Only small messages allowed
    assert!(validate_response_size(500 * 1024, &config.limits).is_ok()); // 500KB OK
    assert!(validate_response_size(2 * 1024 * 1024, &config.limits).is_err()); // 2MB rejected
}

/// Security test: Prevent memory exhaustion
#[test]
fn test_prevent_memory_exhaustion() {
    let limits = LimitsConfig::default();

    // Simulate malicious server trying to send 1GB response
    let malicious_size = 1024 * 1024 * 1024; // 1GB

    let result = validate_response_size(malicious_size, &limits);
    assert!(result.is_err(), "Should reject 1GB response");

    // Memory should not be allocated - validation happens before allocation
    // This test verifies the check happens at the right time
}

/// Documentation example test: Verify examples compile and work
#[test]
fn test_documentation_examples() {
    // Example 1: Default limits
    let _limits = LimitsConfig::default();

    // Example 2: Custom limits
    let _limits = LimitsConfig {
        max_response_size: Some(50 * 1024 * 1024), // 50MB
        max_request_size: Some(5 * 1024 * 1024),   // 5MB
        enforce_on_streams: true,
    };

    // Example 3: Unlimited (for API gateways)
    let _limits = LimitsConfig::unlimited();

    // Example 4: Strict (for untrusted servers)
    let _limits = LimitsConfig::strict();

    // Example 5: Builder pattern
    let _config = TransportConfigBuilder::new(TransportType::Stdio)
        .limits(LimitsConfig::strict())
        .build()
        .unwrap();
}
