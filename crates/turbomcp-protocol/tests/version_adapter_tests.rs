//! Comprehensive end-to-end tests for the multi-version adapter system.
//!
//! Covers: ProtocolVersion serde, adapter filtering, method validation,
//! negotiation, and backward compatibility.

use serde_json::{Value, json};
use turbomcp_protocol::versioning::adapter::*;
use turbomcp_types::ProtocolVersion;

// =============================================================================
// ProtocolVersion Serde Round-Trip Tests
// =============================================================================

#[test]
fn test_serde_roundtrip_v2025_06_18() {
    let v = ProtocolVersion::V2025_06_18;
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, "\"2025-06-18\"");
    let parsed: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ProtocolVersion::V2025_06_18);
}

#[test]
fn test_serde_roundtrip_v2025_11_25() {
    let v = ProtocolVersion::V2025_11_25;
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, "\"2025-11-25\"");
    let parsed: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ProtocolVersion::V2025_11_25);
}

#[test]
fn test_serde_roundtrip_draft() {
    let v = ProtocolVersion::Draft;
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, "\"DRAFT-2026-v1\"");
    let parsed: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ProtocolVersion::Draft);
}

#[test]
fn test_serde_roundtrip_unknown() {
    let v = ProtocolVersion::Unknown("custom-2099".into());
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(json, "\"custom-2099\"");
    let parsed: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, v);
}

#[test]
fn test_serde_deserialize_known_not_unknown() {
    // Deserializing a known version string must produce the enum variant, not Unknown
    let parsed: ProtocolVersion = serde_json::from_str("\"2025-11-25\"").unwrap();
    assert!(matches!(parsed, ProtocolVersion::V2025_11_25));
    assert!(!matches!(parsed, ProtocolVersion::Unknown(_)));
}

// =============================================================================
// ProtocolVersion Ordering Tests
// =============================================================================

#[test]
fn test_ordering_known_versions() {
    assert!(ProtocolVersion::V2025_06_18 < ProtocolVersion::V2025_11_25);
    assert!(ProtocolVersion::V2025_11_25 < ProtocolVersion::Draft);
    assert!(ProtocolVersion::V2025_06_18 < ProtocolVersion::Draft);
}

#[test]
fn test_ordering_unknown_after_all_known() {
    let unknown = ProtocolVersion::Unknown("anything".into());
    assert!(ProtocolVersion::V2025_06_18 < unknown);
    assert!(ProtocolVersion::V2025_11_25 < unknown);
    assert!(ProtocolVersion::Draft < unknown);
}

#[test]
fn test_ordering_unknown_variants_lexicographic() {
    let a = ProtocolVersion::Unknown("aaa".into());
    let b = ProtocolVersion::Unknown("zzz".into());
    assert!(a < b);
    assert!(a != b);
    // Verify Ord and PartialEq are consistent
    assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
}

#[test]
fn test_ordering_same_unknown_equal() {
    let a = ProtocolVersion::Unknown("same".into());
    let b = ProtocolVersion::Unknown("same".into());
    assert_eq!(a, b);
    assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
}

// =============================================================================
// ProtocolVersion From/Display/PartialEq Tests
// =============================================================================

#[test]
fn test_from_str_known_versions() {
    assert_eq!(
        ProtocolVersion::from("2025-06-18"),
        ProtocolVersion::V2025_06_18
    );
    assert_eq!(
        ProtocolVersion::from("2025-11-25"),
        ProtocolVersion::V2025_11_25
    );
    assert_eq!(
        ProtocolVersion::from("DRAFT-2026-v1"),
        ProtocolVersion::Draft
    );
}

#[test]
fn test_from_str_unknown_versions() {
    let v = ProtocolVersion::from("2099-01-01");
    assert!(matches!(v, ProtocolVersion::Unknown(_)));
    assert_eq!(v.as_str(), "2099-01-01");
}

#[test]
fn test_from_str_edge_cases() {
    // Empty string
    let v = ProtocolVersion::from("");
    assert!(matches!(v, ProtocolVersion::Unknown(_)));
    assert_eq!(v.as_str(), "");

    // Whitespace
    let v = ProtocolVersion::from(" 2025-11-25 ");
    assert!(matches!(v, ProtocolVersion::Unknown(_)));

    // Case sensitivity
    let v = ProtocolVersion::from("draft-2026-v1");
    assert!(matches!(v, ProtocolVersion::Unknown(_)));
}

#[test]
fn test_display() {
    assert_eq!(ProtocolVersion::V2025_06_18.to_string(), "2025-06-18");
    assert_eq!(ProtocolVersion::V2025_11_25.to_string(), "2025-11-25");
    assert_eq!(ProtocolVersion::Draft.to_string(), "DRAFT-2026-v1");
    assert_eq!(
        ProtocolVersion::Unknown("custom".into()).to_string(),
        "custom"
    );
}

#[test]
fn test_partial_eq_str() {
    assert!(ProtocolVersion::V2025_11_25 == "2025-11-25");
    assert!("2025-11-25" == ProtocolVersion::V2025_11_25);
    assert!(ProtocolVersion::V2025_06_18 == "2025-06-18");
    assert!(ProtocolVersion::Draft == "DRAFT-2026-v1");
    assert!(ProtocolVersion::V2025_11_25 != "2025-06-18");
}

#[test]
fn test_is_stable_vs_is_known() {
    assert!(ProtocolVersion::V2025_06_18.is_stable());
    assert!(ProtocolVersion::V2025_06_18.is_known());

    assert!(ProtocolVersion::V2025_11_25.is_stable());
    assert!(ProtocolVersion::V2025_11_25.is_known());

    // Draft is known but NOT stable
    assert!(!ProtocolVersion::Draft.is_stable());
    assert!(ProtocolVersion::Draft.is_known());

    // Unknown is neither
    let u = ProtocolVersion::Unknown("x".into());
    assert!(!u.is_stable());
    assert!(!u.is_known());
}

#[test]
fn test_default_is_latest() {
    assert_eq!(ProtocolVersion::default(), ProtocolVersion::V2025_11_25);
    assert_eq!(ProtocolVersion::default(), ProtocolVersion::LATEST.clone());
}

#[test]
fn test_stable_constant_contents() {
    assert_eq!(
        ProtocolVersion::STABLE,
        &[ProtocolVersion::V2025_06_18, ProtocolVersion::V2025_11_25]
    );
}

#[test]
fn test_into_string() {
    let s: String = ProtocolVersion::V2025_11_25.into();
    assert_eq!(s, "2025-11-25");

    let s: String = ProtocolVersion::Unknown("custom".into()).into();
    assert_eq!(s, "custom");
}

// =============================================================================
// Adapter Registry Tests
// =============================================================================

#[test]
fn test_adapter_registry_all_versions() {
    let cases = vec![
        (ProtocolVersion::V2025_06_18, "2025-06-18"),
        (ProtocolVersion::V2025_11_25, "2025-11-25"),
        (ProtocolVersion::Draft, "DRAFT-2026-v1"),
    ];
    for (version, expected_str) in cases {
        let adapter = adapter_for_version(&version);
        assert_eq!(adapter.version(), &version);
        assert_eq!(adapter.version().as_str(), expected_str);
    }
}

#[test]
fn test_adapter_registry_unknown_fallback() {
    let adapter = adapter_for_version(&ProtocolVersion::Unknown("future".into()));
    assert_eq!(adapter.version(), &ProtocolVersion::V2025_11_25);
}

// =============================================================================
// V2025_06_18 Adapter — Capability Filtering
// =============================================================================

#[test]
fn test_v2025_06_18_strips_elicitation_url_capability() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "protocolVersion": "2025-06-18",
        "serverInfo": { "name": "test", "version": "1.0" },
        "capabilities": {
            "tools": { "listChanged": true },
            "elicitation": { "form": {}, "url": {} },
            "sampling": { "tools": {}, "context": {} }
        }
    });

    let filtered = adapter.filter_result("initialize", result);
    let caps = &filtered["capabilities"];

    // elicitation.url should be stripped
    assert!(
        caps["elicitation"].get("form").is_some(),
        "form should remain"
    );
    assert!(
        caps["elicitation"].get("url").is_none(),
        "url should be stripped"
    );

    // sampling.tools should be stripped
    assert!(
        caps["sampling"].get("tools").is_none(),
        "sampling.tools should be stripped"
    );
    // sampling.context existed in 2025-06-18 — should NOT be stripped
    assert!(
        caps["sampling"].get("context").is_some(),
        "sampling.context should remain"
    );
}

#[test]
fn test_v2025_06_18_strips_tasks_capability() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "protocolVersion": "2025-06-18",
        "serverInfo": { "name": "test", "version": "1.0" },
        "capabilities": {
            "tools": { "listChanged": true },
            "tasks": { "list": {}, "cancel": {} }
        }
    });

    let filtered = adapter.filter_result("initialize", result);
    assert!(filtered["capabilities"].get("tasks").is_none());
    assert!(filtered["capabilities"].get("tools").is_some());
}

// =============================================================================
// V2025_06_18 Adapter — Result Filtering
// =============================================================================

#[test]
fn test_v2025_06_18_strips_all_tool_fields() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "tools": [{
            "name": "my-tool",
            "description": "A tool",
            "inputSchema": { "type": "object" },
            "icons": [{ "src": "https://example.com/icon.png" }],
            "execution": { "taskSupport": "optional" },
            "outputSchema": { "type": "object" },
            "title": "My Tool",
            "annotations": { "audience": ["user"] }
        }]
    });

    let filtered = adapter.filter_result("tools/list", result);
    let tool = &filtered["tools"][0];

    // Should be stripped (new in 2025-11-25)
    assert!(tool.get("icons").is_none(), "icons must be stripped");
    assert!(
        tool.get("execution").is_none(),
        "execution must be stripped"
    );
    assert!(
        tool.get("outputSchema").is_none(),
        "outputSchema must be stripped"
    );

    // Should remain (existed in 2025-06-18)
    assert!(tool.get("name").is_some(), "name must remain");
    assert!(tool.get("description").is_some(), "description must remain");
    assert!(tool.get("inputSchema").is_some(), "inputSchema must remain");
    assert!(tool.get("title").is_some(), "title must remain");
    assert!(tool.get("annotations").is_some(), "annotations must remain");
}

#[test]
fn test_v2025_06_18_strips_all_server_info_fields() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "protocolVersion": "2025-06-18",
        "serverInfo": {
            "name": "my-server",
            "version": "1.0.0",
            "title": "My Server",
            "description": "A great server",
            "icons": [{ "src": "https://example.com/icon.png" }],
            "websiteUrl": "https://example.com"
        },
        "capabilities": {}
    });

    let filtered = adapter.filter_result("initialize", result);
    let info = &filtered["serverInfo"];

    // Should be stripped
    assert!(
        info.get("description").is_none(),
        "description must be stripped"
    );
    assert!(info.get("icons").is_none(), "icons must be stripped");
    assert!(
        info.get("websiteUrl").is_none(),
        "websiteUrl must be stripped"
    );

    // Should remain
    assert!(info.get("name").is_some(), "name must remain");
    assert!(info.get("version").is_some(), "version must remain");
    assert!(info.get("title").is_some(), "title must remain");
}

#[test]
fn test_v2025_06_18_strips_prompt_icons() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "prompts": [{
            "name": "summarize",
            "description": "Summarize text",
            "icons": [{ "src": "https://example.com/icon.png" }]
        }]
    });

    let filtered = adapter.filter_result("prompts/list", result);
    let prompt = &filtered["prompts"][0];
    assert!(prompt.get("icons").is_none());
    assert!(prompt.get("name").is_some());
    assert!(prompt.get("description").is_some());
}

#[test]
fn test_v2025_06_18_strips_resource_icons() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "resources": [{
            "uri": "file:///test.txt",
            "name": "test",
            "mimeType": "text/plain",
            "icons": [{ "src": "https://example.com/icon.png" }]
        }]
    });

    let filtered = adapter.filter_result("resources/list", result);
    let resource = &filtered["resources"][0];
    assert!(resource.get("icons").is_none());
    assert!(resource.get("uri").is_some());
    assert!(resource.get("name").is_some());
    assert!(resource.get("mimeType").is_some());
}

#[test]
fn test_v2025_06_18_passthrough_for_other_methods() {
    let adapter = V2025_06_18Adapter;
    let result = json!({
        "content": [{ "type": "text", "text": "hello" }]
    });

    // tools/call, resources/read, etc. pass through unchanged
    let filtered = adapter.filter_result("tools/call", result.clone());
    assert_eq!(filtered, result);
}

// =============================================================================
// V2025_06_18 Adapter — Method Validation
// =============================================================================

#[test]
fn test_v2025_06_18_accepts_all_2025_06_18_methods() {
    let adapter = V2025_06_18Adapter;
    let methods = [
        "initialize",
        "ping",
        "tools/list",
        "tools/call",
        "resources/list",
        "resources/read",
        "resources/subscribe",
        "resources/unsubscribe",
        "prompts/list",
        "prompts/get",
        "completion/complete",
        "logging/setLevel",
        "notifications/initialized",
        "notifications/cancelled",
        "notifications/progress",
        "notifications/message",
        "notifications/resources/updated",
        "notifications/resources/list_changed",
        "notifications/tools/list_changed",
        "notifications/prompts/list_changed",
        "notifications/roots/list_changed",
        "roots/list",
        "sampling/createMessage",
        "elicitation/create",
    ];
    for method in methods {
        assert!(
            adapter.validate_method(method).is_ok(),
            "Method '{method}' should be valid in 2025-06-18"
        );
    }
}

#[test]
fn test_v2025_06_18_rejects_2025_11_25_only_methods() {
    let adapter = V2025_06_18Adapter;
    let methods = [
        "tasks/get",
        "tasks/result",
        "tasks/list",
        "tasks/cancel",
        "notifications/tasks/status",
        "notifications/elicitation/complete",
    ];
    for method in methods {
        assert!(
            adapter.validate_method(method).is_err(),
            "Method '{method}' should be rejected in 2025-06-18"
        );
    }
}

// =============================================================================
// V2025_11_25 Adapter — Pass-Through
// =============================================================================

#[test]
fn test_v2025_11_25_passthrough_all_methods() {
    let adapter = V2025_11_25Adapter;
    let methods = [
        "initialize",
        "tools/list",
        "tools/call",
        "tasks/get",
        "tasks/list",
        "notifications/tasks/status",
    ];
    for method in methods {
        assert!(adapter.validate_method(method).is_ok());
    }
}

#[test]
fn test_v2025_11_25_passthrough_result() {
    let adapter = V2025_11_25Adapter;
    let result = json!({
        "tools": [{
            "name": "tool",
            "icons": [{}],
            "execution": {},
            "outputSchema": {}
        }]
    });
    let filtered = adapter.filter_result("tools/list", result.clone());
    assert_eq!(filtered, result);
}

// =============================================================================
// Draft Adapter — Pass-Through
// =============================================================================

#[test]
fn test_draft_accepts_all_methods() {
    let adapter = DraftAdapter;
    assert!(adapter.validate_method("tools/list").is_ok());
    assert!(adapter.validate_method("tasks/get").is_ok());
    assert!(adapter.validate_method("some/future/method").is_ok());
}

// =============================================================================
// Method Set Consistency
// =============================================================================

#[test]
fn test_method_sets_superset_relationship() {
    let v06 = V2025_06_18Adapter;
    let v11 = V2025_11_25Adapter;

    let methods_06 = v06.supported_methods();
    let methods_11 = v11.supported_methods();

    // 2025-11-25 should contain all 2025-06-18 methods
    for method in methods_06.iter() {
        assert!(
            methods_11.contains(method),
            "2025-11-25 must contain '{method}' from 2025-06-18"
        );
    }

    // 2025-11-25 should have MORE methods than 2025-06-18
    assert!(methods_11.len() > methods_06.len());
}

// =============================================================================
// ElicitationCapabilities Backward Compat
// =============================================================================

#[test]
fn test_elicitation_empty_object_defaults_to_form() {
    use turbomcp_protocol::types::capabilities::ElicitationCapabilities;

    // Empty object (backward compat): {} → form support
    let empty = ElicitationCapabilities::default();
    assert!(empty.supports_form(), "empty caps default to form");
    assert!(!empty.supports_url(), "empty caps don't support URL");

    // Serde round-trip: {} should deserialize to default
    let json = "{}";
    let parsed: ElicitationCapabilities = serde_json::from_str(json).unwrap();
    assert!(parsed.supports_form());
    assert!(!parsed.supports_url());
}

#[test]
fn test_elicitation_full_capabilities() {
    use turbomcp_protocol::types::capabilities::ElicitationCapabilities;

    let full = ElicitationCapabilities::full();
    assert!(full.supports_form());
    assert!(full.supports_url());

    // Serde round-trip
    let json = serde_json::to_string(&full).unwrap();
    let parsed: ElicitationCapabilities = serde_json::from_str(&json).unwrap();
    assert!(parsed.supports_form());
    assert!(parsed.supports_url());
}

#[test]
fn test_elicitation_form_only() {
    use turbomcp_protocol::types::capabilities::ElicitationCapabilities;

    let form = ElicitationCapabilities::form_only();
    assert!(form.supports_form());
    assert!(!form.supports_url());

    // Should serialize with form but no url
    let json = serde_json::to_value(&form).unwrap();
    assert!(json.get("form").is_some());
    assert!(json.get("url").is_none());
}

// =============================================================================
// End-to-End Initialize Response Filtering
// =============================================================================

#[test]
fn test_e2e_initialize_response_v2025_06_18() {
    let adapter = adapter_for_version(&ProtocolVersion::V2025_06_18);

    // Simulate a full initialize response with all 2025-11-25 features
    let response = json!({
        "protocolVersion": "2025-06-18",
        "serverInfo": {
            "name": "my-server",
            "version": "1.0.0",
            "title": "My Server",
            "description": "A great server",
            "icons": [{ "src": "https://example.com/icon.png", "mimeType": "image/png" }],
            "websiteUrl": "https://example.com"
        },
        "capabilities": {
            "tools": { "listChanged": true },
            "resources": { "subscribe": true, "listChanged": true },
            "prompts": { "listChanged": true },
            "logging": {},
            "completions": {},
            "tasks": {
                "list": {},
                "cancel": {},
                "requests": { "tools": { "call": {} } }
            },
            "elicitation": { "form": {}, "url": {} },
            "sampling": { "tools": {} }
        },
        "instructions": "Use tools wisely"
    });

    let filtered = adapter.filter_result("initialize", response);

    // serverInfo: description, icons, websiteUrl stripped
    let info = &filtered["serverInfo"];
    assert_eq!(info["name"], "my-server");
    assert_eq!(info["version"], "1.0.0");
    assert_eq!(info["title"], "My Server");
    assert!(info.get("description").is_none());
    assert!(info.get("icons").is_none());
    assert!(info.get("websiteUrl").is_none());

    // capabilities: tasks stripped, elicitation.url stripped, sampling.tools stripped
    let caps = &filtered["capabilities"];
    assert!(caps.get("tools").is_some());
    assert!(caps.get("resources").is_some());
    assert!(caps.get("prompts").is_some());
    assert!(caps.get("logging").is_some());
    assert!(caps.get("completions").is_some());
    assert!(caps.get("tasks").is_none());
    assert!(caps["elicitation"].get("form").is_some());
    assert!(caps["elicitation"].get("url").is_none());
    assert!(caps["sampling"].get("tools").is_none());

    // instructions: should remain (existed in 2025-06-18)
    assert_eq!(filtered["instructions"], "Use tools wisely");
}

#[test]
fn test_e2e_tools_list_response_v2025_06_18() {
    let adapter = adapter_for_version(&ProtocolVersion::V2025_06_18);

    let response = json!({
        "tools": [
            {
                "name": "calculator",
                "title": "Calculator",
                "description": "Performs arithmetic",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string" }
                    }
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "result": { "type": "number" }
                    }
                },
                "icons": [{ "src": "data:image/svg+xml;base64,..." }],
                "execution": { "taskSupport": "optional" },
                "annotations": {
                    "audience": ["user"],
                    "priority": 1.0
                }
            },
            {
                "name": "search",
                "description": "Web search",
                "inputSchema": { "type": "object" }
            }
        ]
    });

    let filtered = adapter.filter_result("tools/list", response);

    // Tool 1: new fields stripped, old fields preserved
    let t1 = &filtered["tools"][0];
    assert_eq!(t1["name"], "calculator");
    assert_eq!(t1["title"], "Calculator");
    assert!(t1.get("outputSchema").is_none());
    assert!(t1.get("icons").is_none());
    assert!(t1.get("execution").is_none());
    assert!(t1.get("annotations").is_some());

    // Tool 2: simple tool unaffected
    let t2 = &filtered["tools"][1];
    assert_eq!(t2["name"], "search");
    assert!(t2.get("description").is_some());
}

// =============================================================================
// Error Response Passthrough
// =============================================================================

#[test]
fn test_adapter_does_not_filter_error_values() {
    let adapter = V2025_06_18Adapter;
    // A null result (as would be the case for errors) should pass through
    let result = Value::Null;
    let filtered = adapter.filter_result("initialize", result.clone());
    assert_eq!(filtered, result);
}
