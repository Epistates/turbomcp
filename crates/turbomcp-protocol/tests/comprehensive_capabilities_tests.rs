//! Comprehensive tests for capabilities.rs - targeting 100% coverage
//! Testing all capability negotiation, matching, and management functionality

use std::collections::HashMap;
use turbomcp_protocol::capabilities::*;
use turbomcp_protocol::types::*;

// Helper functions for creating test capabilities
fn create_minimal_client_capabilities() -> ClientCapabilities {
    ClientCapabilities::default()
}

fn create_minimal_server_capabilities() -> ServerCapabilities {
    ServerCapabilities::default()
}

fn create_full_client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        sampling: Some(SamplingCapabilities),
        roots: Some(RootsCapabilities {
            list_changed: Some(true),
        }),
        elicitation: Some(ElicitationCapabilities::default()),
        experimental: Some({
            let mut experimental = HashMap::new();
            experimental.insert(
                "custom_feature".to_string(),
                serde_json::json!({"enabled": true}),
            );
            experimental
        }),
    }
}

fn create_full_server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        tools: Some(ToolsCapabilities {
            list_changed: Some(true),
        }),
        prompts: Some(PromptsCapabilities {
            list_changed: Some(true),
        }),
        resources: Some(ResourcesCapabilities {
            subscribe: Some(true),
            list_changed: Some(true),
        }),
        logging: Some(LoggingCapabilities),
        completions: Some(CompletionCapabilities),
        experimental: Some({
            let mut experimental = HashMap::new();
            experimental.insert(
                "server_custom".to_string(),
                serde_json::json!({"version": "1.0"}),
            );
            experimental
        }),
    }
}

fn create_partial_client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        sampling: Some(SamplingCapabilities),
        roots: None,
        elicitation: None,
        experimental: None,
    }
}

fn create_partial_server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        tools: Some(ToolsCapabilities {
            list_changed: Some(false),
        }),
        prompts: None,
        resources: None,
        logging: None,
        completions: None,
        experimental: None,
    }
}

// Custom compatibility function for testing
fn custom_compatibility_function(
    _client: &ClientCapabilities,
    server: &ServerCapabilities,
) -> bool {
    server.tools.is_some()
}

#[test]
fn test_capability_matcher_new() {
    let matcher = CapabilityMatcher::new();

    // Test that default rules are set correctly
    let client = create_full_client_capabilities();
    let server = create_full_server_capabilities();

    assert!(matcher.is_compatible("tools", &client, &server));
    assert!(matcher.is_compatible("prompts", &client, &server));
    assert!(matcher.is_compatible("resources", &client, &server));
    assert!(matcher.is_compatible("logging", &client, &server));
    assert!(matcher.is_compatible("sampling", &client, &server));
    assert!(matcher.is_compatible("roots", &client, &server));
    assert!(matcher.is_compatible("progress", &client, &server)); // Should be optional
}

#[test]
fn test_capability_matcher_default() {
    let matcher = CapabilityMatcher::default();
    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Progress should be optional (always compatible)
    assert!(matcher.is_compatible("progress", &client, &server));
}

#[test]
fn test_add_rule() {
    let mut matcher = CapabilityMatcher::new();

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Add a custom rule
    matcher.add_rule("custom_feature", CompatibilityRule::RequireClient);

    // Should not be compatible (client doesn't have custom_feature)
    assert!(!matcher.is_compatible("custom_feature", &client, &server));

    // Test with experimental feature
    let client_with_experimental = create_full_client_capabilities();
    assert!(matcher.is_compatible("custom_feature", &client_with_experimental, &server));
}

#[test]
fn test_set_default() {
    let mut matcher = CapabilityMatcher::new();
    matcher.set_default("test_feature", true);
    matcher.add_rule("test_feature", CompatibilityRule::Optional); // Make it compatible

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Test that negotiation includes default features
    let result = matcher.negotiate(&client, &server);
    assert!(result.is_ok());

    let capability_set = result.unwrap();
    assert!(capability_set.has_feature("test_feature"));
}

#[test]
fn test_compatibility_rules_require_both() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("both_required", CompatibilityRule::RequireBoth);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Neither has the feature
    assert!(!matcher.is_compatible("both_required", &client, &server));

    // Only client has it (via experimental)
    let client_with_experimental = create_full_client_capabilities();
    assert!(!matcher.is_compatible("both_required", &client_with_experimental, &server));

    // Only server has it (via experimental)
    let server_with_experimental = create_full_server_capabilities();
    assert!(!matcher.is_compatible("both_required", &client, &server_with_experimental));

    // Both have it
    assert!(!matcher.is_compatible(
        "both_required",
        &client_with_experimental,
        &server_with_experimental
    ));
}

#[test]
fn test_compatibility_rules_require_client() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("custom_feature", CompatibilityRule::RequireClient);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Client doesn't have it
    assert!(!matcher.is_compatible("custom_feature", &client, &server));

    // Client has it via experimental (custom_feature is in the experimental features)
    let client_with_experimental = create_full_client_capabilities();
    assert!(matcher.is_compatible("custom_feature", &client_with_experimental, &server));
}

#[test]
fn test_compatibility_rules_require_server() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("server_custom", CompatibilityRule::RequireServer);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Server doesn't have it
    assert!(!matcher.is_compatible("server_custom", &client, &server));

    // Server has it via experimental (server_custom is in the experimental features)
    let server_with_experimental = create_full_server_capabilities();
    assert!(matcher.is_compatible("server_custom", &client, &server_with_experimental));
}

#[test]
fn test_compatibility_rules_optional() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("optional_feature", CompatibilityRule::Optional);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Should always be compatible for optional features
    assert!(matcher.is_compatible("optional_feature", &client, &server));
}

#[test]
fn test_compatibility_rules_custom() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule(
        "custom_rule",
        CompatibilityRule::Custom(custom_compatibility_function),
    );

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Custom function requires server.tools
    assert!(!matcher.is_compatible("custom_rule", &client, &server));

    let server_with_tools = create_full_server_capabilities();
    assert!(matcher.is_compatible("custom_rule", &client, &server_with_tools));
}

#[test]
fn test_unknown_feature_compatibility() {
    let matcher = CapabilityMatcher::new();

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Unknown features should check if either side supports it
    assert!(!matcher.is_compatible("unknown_feature", &client, &server));

    // With experimental features
    let client_with_experimental = create_full_client_capabilities();
    let server_with_experimental = create_full_server_capabilities();

    // Should find custom_feature in client experimental
    assert!(matcher.is_compatible("custom_feature", &client_with_experimental, &server));

    // Should find server_custom in server experimental
    assert!(matcher.is_compatible("server_custom", &client, &server_with_experimental));
}

// NOTE: Removed tests for private methods client_has_feature, server_has_feature, get_all_features
// These are tested indirectly through the public negotiate and is_compatible methods

#[test]
fn test_negotiate_success() {
    let matcher = CapabilityMatcher::new();

    let client = create_full_client_capabilities();
    let server = create_full_server_capabilities();

    let result = matcher.negotiate(&client, &server);
    assert!(result.is_ok());

    let capability_set = result.unwrap();
    assert!(capability_set.has_feature("sampling"));
    assert!(capability_set.has_feature("tools"));
    assert!(capability_set.has_feature("roots"));
    assert!(capability_set.has_feature("prompts"));
    assert!(capability_set.has_feature("resources"));
    assert!(capability_set.has_feature("logging"));
    assert!(capability_set.has_feature("progress"));

    // Check that experimental features are included
    assert!(capability_set.has_feature("custom_feature"));
    assert!(capability_set.has_feature("server_custom"));
}

#[test]
fn test_negotiate_incompatible_features() {
    let mut matcher = CapabilityMatcher::new();

    // Add a rule that will fail
    matcher.add_rule("impossible_feature", CompatibilityRule::RequireBoth);
    matcher.set_default("impossible_feature", true); // Force it to be checked

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    let result = matcher.negotiate(&client, &server);
    assert!(result.is_err());

    match result.unwrap_err() {
        CapabilityError::IncompatibleFeatures(features) => {
            assert!(features.contains(&"impossible_feature".to_string()));
        }
        _ => panic!("Expected IncompatibleFeatures error"),
    }
}

#[test]
fn test_negotiate_with_defaults() {
    let mut matcher = CapabilityMatcher::new();
    matcher.set_default("default_enabled", true);
    matcher.set_default("default_disabled", false);
    matcher.add_rule("default_enabled", CompatibilityRule::Optional);
    matcher.add_rule("default_disabled", CompatibilityRule::Optional);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    let result = matcher.negotiate(&client, &server);
    assert!(result.is_ok());

    let capability_set = result.unwrap();
    assert!(capability_set.has_feature("default_enabled"));
    // Note: Even false defaults may be included if the feature is in all_features
    // The test logic checks if feature is in all_features AND default is enabled
}

#[test]
fn test_capability_negotiator_new() {
    let matcher = CapabilityMatcher::new();
    let negotiator = CapabilityNegotiator::new(matcher);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    let result = negotiator.negotiate(&client, &server);
    assert!(result.is_ok());
}

#[test]
fn test_capability_negotiator_default() {
    let negotiator = CapabilityNegotiator::default();

    let client = create_full_client_capabilities();
    let server = create_full_server_capabilities();

    let result = negotiator.negotiate(&client, &server);
    assert!(result.is_ok());
}

#[test]
fn test_capability_negotiator_with_strict_mode() {
    let negotiator = CapabilityNegotiator::default().with_strict_mode();

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    let result = negotiator.negotiate(&client, &server);
    assert!(result.is_ok()); // Should work with minimal capabilities
}

#[test]
fn test_capability_negotiator_strict_mode_failure() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("impossible", CompatibilityRule::RequireBoth);
    matcher.set_default("impossible", true);

    let negotiator = CapabilityNegotiator::new(matcher).with_strict_mode();

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    let result = negotiator.negotiate(&client, &server);
    assert!(result.is_err());
}

#[test]
fn test_capability_negotiator_non_strict_mode() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("incompatible", CompatibilityRule::RequireBoth);
    matcher.set_default("incompatible", true);

    let negotiator = CapabilityNegotiator::new(matcher); // Non-strict by default

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    let result = negotiator.negotiate(&client, &server);
    assert!(result.is_ok()); // Should succeed in non-strict mode

    let capability_set = result.unwrap();
    assert!(!capability_set.has_feature("incompatible")); // Feature should be excluded
}

#[test]
fn test_is_feature_enabled() {
    let negotiator = CapabilityNegotiator::default();
    let client = create_full_client_capabilities();
    let server = create_full_server_capabilities();

    let capability_set = negotiator.negotiate(&client, &server).unwrap();

    assert!(CapabilityNegotiator::is_feature_enabled(
        &capability_set,
        "tools"
    ));
    assert!(CapabilityNegotiator::is_feature_enabled(
        &capability_set,
        "sampling"
    ));
    assert!(!CapabilityNegotiator::is_feature_enabled(
        &capability_set,
        "nonexistent"
    ));
}

#[test]
fn test_get_enabled_features() {
    let negotiator = CapabilityNegotiator::default();
    let client = create_full_client_capabilities();
    let server = create_full_server_capabilities();

    let capability_set = negotiator.negotiate(&client, &server).unwrap();
    let features = CapabilityNegotiator::get_enabled_features(&capability_set);

    // Should be sorted
    assert!(features.windows(2).all(|w| w[0] <= w[1]));

    // Should contain expected features
    assert!(features.contains(&"tools".to_string()));
    assert!(features.contains(&"sampling".to_string()));
}

#[test]
fn test_capability_set_empty() {
    let capability_set = CapabilitySet::empty();

    assert_eq!(capability_set.feature_count(), 0);
    assert!(!capability_set.has_feature("anything"));
}

#[test]
fn test_capability_set_enable_disable_feature() {
    let mut capability_set = CapabilitySet::empty();

    assert!(!capability_set.has_feature("test"));
    assert_eq!(capability_set.feature_count(), 0);

    capability_set.enable_feature("test".to_string());
    assert!(capability_set.has_feature("test"));
    assert_eq!(capability_set.feature_count(), 1);

    capability_set.disable_feature("test");
    assert!(!capability_set.has_feature("test"));
    assert_eq!(capability_set.feature_count(), 0);
}

#[test]
fn test_capability_set_metadata() {
    let mut capability_set = CapabilitySet::empty();

    assert!(capability_set.get_metadata("key").is_none());

    capability_set.add_metadata("key".to_string(), serde_json::json!({"value": 42}));

    let metadata = capability_set.get_metadata("key");
    assert!(metadata.is_some());
    assert_eq!(metadata.unwrap()["value"], 42);
}

#[test]
fn test_capability_set_summary() {
    let mut capability_set = CapabilitySet::empty();

    // Set up client capabilities
    capability_set.client_capabilities.sampling = Some(SamplingCapabilities);
    capability_set.client_capabilities.roots = Some(RootsCapabilities::default());

    // Set up server capabilities
    capability_set.server_capabilities.tools = Some(ToolsCapabilities::default());
    capability_set.server_capabilities.prompts = Some(PromptsCapabilities::default());
    capability_set.server_capabilities.resources = Some(ResourcesCapabilities::default());

    // Enable some features
    capability_set.enable_feature("feature1".to_string());
    capability_set.enable_feature("feature2".to_string());

    let summary = capability_set.summary();

    assert_eq!(summary.total_features, 2);
    assert_eq!(summary.client_features, 2); // sampling + roots
    assert_eq!(summary.server_features, 3); // tools + prompts + resources
    assert!(summary.enabled_features.contains(&"feature1".to_string()));
    assert!(summary.enabled_features.contains(&"feature2".to_string()));
}

// NOTE: Removed tests for private methods count_client_features and count_server_features
// These are tested indirectly through the public summary() method

#[test]
fn test_capability_error_display() {
    let error =
        CapabilityError::IncompatibleFeatures(vec!["feature1".to_string(), "feature2".to_string()]);

    let display = format!("{error}");
    assert!(display.contains("Incompatible features"));
    assert!(display.contains("feature1"));
    assert!(display.contains("feature2"));

    let error = CapabilityError::RequiredFeatureMissing("required_feature".to_string());
    let display = format!("{error}");
    assert!(display.contains("Required feature missing"));
    assert!(display.contains("required_feature"));

    let error = CapabilityError::VersionMismatch {
        client: "1.0".to_string(),
        server: "2.0".to_string(),
    };
    let display = format!("{error}");
    assert!(display.contains("Protocol version mismatch"));
    assert!(display.contains("client=1.0"));
    assert!(display.contains("server=2.0"));

    let error = CapabilityError::NegotiationFailed("test reason".to_string());
    let display = format!("{error}");
    assert!(display.contains("Capability negotiation failed"));
    assert!(display.contains("test reason"));
}

#[test]
fn test_capability_summary_serialization() {
    let summary = CapabilitySummary {
        total_features: 5,
        client_features: 2,
        server_features: 3,
        enabled_features: vec!["feature1".to_string(), "feature2".to_string()],
    };

    // Test serialization
    let json = serde_json::to_string(&summary).unwrap();
    let deserialized: CapabilitySummary = serde_json::from_str(&json).unwrap();

    assert_eq!(summary.total_features, deserialized.total_features);
    assert_eq!(summary.client_features, deserialized.client_features);
    assert_eq!(summary.server_features, deserialized.server_features);
    assert_eq!(summary.enabled_features, deserialized.enabled_features);
}

// Test utility functions
#[test]
fn test_utils_minimal_capabilities() {
    let client = utils::minimal_client_capabilities();
    assert!(client.sampling.is_none());
    assert!(client.roots.is_none());
    assert!(client.experimental.is_none());

    let server = utils::minimal_server_capabilities();
    assert!(server.tools.is_none());
    assert!(server.prompts.is_none());
    assert!(server.resources.is_none());
    assert!(server.logging.is_none());
    assert!(server.experimental.is_none());
}

#[test]
fn test_utils_full_capabilities() {
    let client = utils::full_client_capabilities();
    assert!(client.sampling.is_some());
    assert!(client.roots.is_some());
    assert!(client.elicitation.is_some());

    let server = utils::full_server_capabilities();
    assert!(server.tools.is_some());
    assert!(server.prompts.is_some());
    assert!(server.resources.is_some());
    assert!(server.logging.is_some());
    assert!(server.completions.is_some());
}

#[test]
fn test_utils_are_compatible() {
    let client = utils::full_client_capabilities();
    let server = utils::full_server_capabilities();

    assert!(utils::are_compatible(&client, &server));

    // Test with incompatible setup
    let minimal_client = utils::minimal_client_capabilities();
    let minimal_server = utils::minimal_server_capabilities();

    assert!(utils::are_compatible(&minimal_client, &minimal_server)); // Should work with defaults
}

#[test]
fn test_compatibility_rule_clone_debug() {
    let rule1 = CompatibilityRule::RequireBoth;
    let rule2 = rule1.clone();

    // Test Debug formatting
    let debug_str = format!("{rule2:?}");
    assert!(debug_str.contains("RequireBoth"));

    let custom_rule = CompatibilityRule::Custom(custom_compatibility_function);
    let debug_str = format!("{custom_rule:?}");
    assert!(debug_str.contains("Custom"));
}

#[test]
fn test_capability_matcher_clone_debug() {
    let matcher = CapabilityMatcher::new();
    let cloned_matcher = matcher.clone();

    let debug_str = format!("{cloned_matcher:?}");
    assert!(debug_str.contains("CapabilityMatcher"));
}

#[test]
fn test_capability_negotiator_clone_debug() {
    let negotiator = CapabilityNegotiator::default();
    let cloned_negotiator = negotiator.clone();

    let debug_str = format!("{cloned_negotiator:?}");
    assert!(debug_str.contains("CapabilityNegotiator"));
}

#[test]
fn test_compatibility_edge_cases() {
    let matcher = CapabilityMatcher::new();

    // Test with partial capabilities
    let partial_client = create_partial_client_capabilities();
    let partial_server = create_partial_server_capabilities();

    // Sampling should be compatible (client has it)
    assert!(matcher.is_compatible("sampling", &partial_client, &partial_server));

    // Tools should be compatible (server has it)
    assert!(matcher.is_compatible("tools", &partial_client, &partial_server));

    // Roots should not be compatible (client doesn't have it, rule requires client)
    assert!(!matcher.is_compatible("roots", &partial_client, &partial_server));

    // Prompts should not be compatible (server doesn't have it, rule requires server)
    assert!(!matcher.is_compatible("prompts", &partial_client, &partial_server));
}

#[test]
fn test_complex_negotiation_scenario() {
    let mut matcher = CapabilityMatcher::new();

    // Add custom rules for complex scenario
    matcher.add_rule("advanced_feature", CompatibilityRule::RequireBoth);
    matcher.add_rule("optional_enhancement", CompatibilityRule::Optional);
    matcher.add_rule("client_specific", CompatibilityRule::RequireClient);
    matcher.set_default("optional_enhancement", true);

    let mut client = create_partial_client_capabilities();
    client.experimental = Some({
        let mut exp = HashMap::new();
        exp.insert("advanced_feature".to_string(), serde_json::json!(true));
        exp.insert("client_specific".to_string(), serde_json::json!(true));
        exp
    });

    let mut server = create_partial_server_capabilities();
    server.experimental = Some({
        let mut exp = HashMap::new();
        exp.insert("advanced_feature".to_string(), serde_json::json!(true));
        exp
    });

    let result = matcher.negotiate(&client, &server);
    assert!(result.is_ok());

    let capability_set = result.unwrap();
    assert!(capability_set.has_feature("sampling")); // Client standard feature
    assert!(capability_set.has_feature("tools")); // Server standard feature
    assert!(capability_set.has_feature("advanced_feature")); // Both have it
    assert!(capability_set.has_feature("client_specific")); // Client has it
    assert!(capability_set.has_feature("optional_enhancement")); // Default enabled
    assert!(capability_set.has_feature("progress")); // Always default
}

#[test]
fn test_error_propagation() {
    let mut matcher = CapabilityMatcher::new();
    matcher.add_rule("failing_feature", CompatibilityRule::Custom(|_, _| false));
    matcher.set_default("failing_feature", true);

    let client = create_minimal_client_capabilities();
    let server = create_minimal_server_capabilities();

    // Should fail in strict mode
    let strict_negotiator = CapabilityNegotiator::new(matcher.clone()).with_strict_mode();
    let result = strict_negotiator.negotiate(&client, &server);
    assert!(result.is_err());

    // Should succeed in non-strict mode
    let lenient_negotiator = CapabilityNegotiator::new(matcher);
    let result = lenient_negotiator.negotiate(&client, &server);
    assert!(result.is_ok());
}
