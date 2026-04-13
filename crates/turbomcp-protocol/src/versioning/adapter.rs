//! Version adapter layer for multi-version MCP protocol support.
//!
//! The adapter sits between the transport and handler layers, transforming
//! outgoing messages and filtering capabilities based on the negotiated
//! protocol version. This allows the codebase to always work with the latest
//! types internally while producing spec-compliant wire output for older clients.
//!
//! # Architecture
//!
//! ```text
//! Transport → (incoming) → Router → Handler
//! Handler → Router → VersionAdapter::filter → Transport (outgoing)
//! ```
//!
//! # Usage
//!
//! ```rust
//! use turbomcp_protocol::versioning::adapter::{VersionAdapter, adapter_for_version};
//! use turbomcp_core::types::core::ProtocolVersion;
//!
//! let adapter = adapter_for_version(&ProtocolVersion::V2025_06_18);
//! assert_eq!(adapter.version(), &ProtocolVersion::V2025_06_18);
//! ```

use serde_json::Value;
use std::collections::HashSet;
use turbomcp_core::types::core::ProtocolVersion;

use crate::types::capabilities::ServerCapabilities;

/// Trait for adapting protocol messages to a specific MCP spec version.
///
/// Implementations strip fields, reject methods, and filter capabilities
/// that don't exist in the target version.
pub trait VersionAdapter: Send + Sync + std::fmt::Debug {
    /// The protocol version this adapter targets.
    fn version(&self) -> &ProtocolVersion;

    /// Filter server capabilities for the target version.
    ///
    /// Removes capabilities that don't exist in the target spec version.
    fn filter_capabilities(&self, caps: ServerCapabilities) -> ServerCapabilities;

    /// Filter an outgoing JSON-RPC result value for the target version.
    ///
    /// Strips fields from the JSON that don't exist in the target spec.
    /// The `method` parameter indicates which RPC method produced this result.
    fn filter_result(&self, method: &str, result: Value) -> Value;

    /// Validate that an incoming method is supported in the target version.
    ///
    /// Returns `Ok(())` if the method exists in the target spec,
    /// or `Err(reason)` if it should be rejected.
    fn validate_method(&self, method: &str) -> Result<(), String>;

    /// Methods that are valid in the target version.
    fn supported_methods(&self) -> &HashSet<&'static str>;
}

// =============================================================================
// MCP 2025-11-25 Adapter (pass-through — current version)
// =============================================================================

/// Adapter for MCP 2025-11-25 (current stable spec).
///
/// Strips draft-only fields such as capability `extensions`.
#[derive(Debug)]
pub struct V2025_11_25Adapter;

impl VersionAdapter for V2025_11_25Adapter {
    fn version(&self) -> &ProtocolVersion {
        &ProtocolVersion::V2025_11_25
    }

    fn filter_capabilities(&self, caps: ServerCapabilities) -> ServerCapabilities {
        let mut caps = caps;
        caps.extensions = None;
        caps
    }

    fn filter_result(&self, method: &str, mut result: Value) -> Value {
        if method == "initialize"
            && let Some(caps) = result.get_mut("capabilities")
        {
            strip_keys(caps, &["extensions"]);
        }
        result
    }

    fn validate_method(&self, _method: &str) -> Result<(), String> {
        Ok(()) // all methods valid
    }

    fn supported_methods(&self) -> &HashSet<&'static str> {
        &METHODS_2025_11_25
    }
}

// =============================================================================
// MCP 2025-06-18 Adapter (strips 2025-11-25 additions)
// =============================================================================

/// Adapter for MCP 2025-06-18 (previous stable spec).
///
/// Strips fields and capabilities that were added in 2025-11-25:
/// - `icons` on Tool, Prompt, Resource, Implementation
/// - `execution` (taskSupport) on Tool
/// - `description`, `websiteUrl` on Implementation/ServerInfo
/// - `tasks` capability
/// - URL mode elicitation (capability and methods)
/// - `outputSchema` on Tool
#[derive(Debug)]
pub struct V2025_06_18Adapter;

impl VersionAdapter for V2025_06_18Adapter {
    fn version(&self) -> &ProtocolVersion {
        &ProtocolVersion::V2025_06_18
    }

    fn filter_capabilities(&self, caps: ServerCapabilities) -> ServerCapabilities {
        let mut caps = caps;
        caps.extensions = None;
        // Tasks didn't exist in 2025-06-18
        #[cfg(feature = "experimental-tasks")]
        {
            caps.tasks = None;
        }
        caps
    }

    fn filter_result(&self, method: &str, mut result: Value) -> Value {
        match method {
            "initialize" => {
                // Strip new serverInfo fields (added in 2025-11-25)
                if let Some(info) = result.get_mut("serverInfo") {
                    strip_keys(info, &["description", "icons", "websiteUrl"]);
                }
                if let Some(caps) = result.get_mut("capabilities") {
                    // Strip tasks capability (new in 2025-11-25)
                    strip_keys(caps, &["tasks", "extensions"]);
                    // Strip url sub-capability from elicitation (new in 2025-11-25)
                    if let Some(elicitation) = caps.get_mut("elicitation") {
                        strip_keys(elicitation, &["url"]);
                    }
                    // Strip tools sub-capability from sampling (new in 2025-11-25)
                    if let Some(sampling) = caps.get_mut("sampling") {
                        strip_keys(sampling, &["tools"]);
                    }
                }
                result
            }
            "tools/list" => {
                strip_from_array(
                    &mut result,
                    "tools",
                    &["icons", "execution", "outputSchema"],
                );
                result
            }
            "prompts/list" => {
                strip_from_array(&mut result, "prompts", &["icons"]);
                result
            }
            "resources/list" => {
                strip_from_array(&mut result, "resources", &["icons"]);
                result
            }
            _ => result,
        }
    }

    fn validate_method(&self, method: &str) -> Result<(), String> {
        if METHODS_2025_11_25_ONLY.contains(method) {
            Err(format!(
                "Method '{method}' is not available in MCP 2025-06-18"
            ))
        } else {
            Ok(())
        }
    }

    fn supported_methods(&self) -> &HashSet<&'static str> {
        &METHODS_2025_06_18
    }
}

// =============================================================================
// Draft Adapter (adds extensions support)
// =============================================================================

/// Adapter for the draft MCP specification (DRAFT-2026-v1).
///
/// Passes through everything from 2025-11-25 plus supports the new
/// `extensions` field on capabilities.
#[derive(Debug)]
pub struct DraftAdapter;

impl VersionAdapter for DraftAdapter {
    fn version(&self) -> &ProtocolVersion {
        &ProtocolVersion::Draft
    }

    fn filter_capabilities(&self, caps: ServerCapabilities) -> ServerCapabilities {
        caps // pass-through — draft is superset of 2025-11-25
    }

    fn filter_result(&self, _method: &str, result: Value) -> Value {
        result // pass-through
    }

    fn validate_method(&self, _method: &str) -> Result<(), String> {
        Ok(()) // all methods valid
    }

    fn supported_methods(&self) -> &HashSet<&'static str> {
        &METHODS_2025_11_25 // draft uses same methods as 2025-11-25
    }
}

// =============================================================================
// Adapter Registry
// =============================================================================

// Static adapter instances — zero-sized types with no state, so these are
// trivially const-constructible and eliminate per-request heap allocation.
static ADAPTER_V2025_06_18: V2025_06_18Adapter = V2025_06_18Adapter;
static ADAPTER_V2025_11_25: V2025_11_25Adapter = V2025_11_25Adapter;
static ADAPTER_DRAFT: DraftAdapter = DraftAdapter;

/// Get the appropriate version adapter for a protocol version.
///
/// Returns a static reference to the adapter for the given version.
/// Unknown versions fall back to the latest stable adapter.
pub fn adapter_for_version(version: &ProtocolVersion) -> &'static dyn VersionAdapter {
    match version {
        ProtocolVersion::V2025_06_18 => &ADAPTER_V2025_06_18,
        ProtocolVersion::V2025_11_25 => &ADAPTER_V2025_11_25,
        ProtocolVersion::Draft => &ADAPTER_DRAFT,
        ProtocolVersion::Unknown(_) => &ADAPTER_V2025_11_25, // fallback
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Strip keys from a JSON object.
fn strip_keys(value: &mut Value, keys: &[&str]) {
    if let Value::Object(map) = value {
        for key in keys {
            map.remove(*key);
        }
    }
}

/// Strip keys from each element in a JSON array within a result object.
fn strip_from_array(result: &mut Value, array_key: &str, keys: &[&str]) {
    if let Some(Value::Array(items)) = result.get_mut(array_key) {
        for item in items.iter_mut() {
            strip_keys(item, keys);
        }
    }
}

// =============================================================================
// Method Sets
// =============================================================================

use std::sync::LazyLock;

/// Methods available in MCP 2025-06-18.
static METHODS_2025_06_18: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
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
    ])
});

/// Methods available in MCP 2025-11-25 (superset of 2025-06-18).
static METHODS_2025_11_25: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut methods = METHODS_2025_06_18.clone();
    methods.extend([
        "tasks/get",
        "tasks/result",
        "tasks/list",
        "tasks/cancel",
        "notifications/tasks/status",
        "notifications/elicitation/complete",
    ]);
    methods
});

/// Methods that exist only in 2025-11-25 (not in 2025-06-18).
static METHODS_2025_11_25_ONLY: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    METHODS_2025_11_25
        .difference(&METHODS_2025_06_18)
        .copied()
        .collect()
});

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_adapter_for_known_versions() {
        let adapter = adapter_for_version(&ProtocolVersion::V2025_06_18);
        assert_eq!(adapter.version(), &ProtocolVersion::V2025_06_18);

        let adapter = adapter_for_version(&ProtocolVersion::V2025_11_25);
        assert_eq!(adapter.version(), &ProtocolVersion::V2025_11_25);

        let adapter = adapter_for_version(&ProtocolVersion::Draft);
        assert_eq!(adapter.version(), &ProtocolVersion::Draft);
    }

    #[test]
    fn test_unknown_version_falls_back() {
        let adapter = adapter_for_version(&ProtocolVersion::Unknown("9999-01-01".into()));
        assert_eq!(adapter.version(), &ProtocolVersion::V2025_11_25);
    }

    #[test]
    fn test_v2025_11_25_passthrough() {
        let adapter = V2025_11_25Adapter;
        let caps = ServerCapabilities::default();
        let filtered = adapter.filter_capabilities(caps.clone());
        assert_eq!(
            serde_json::to_string(&filtered).unwrap(),
            serde_json::to_string(&caps).unwrap()
        );
    }

    #[test]
    fn test_v2025_11_25_strips_draft_extensions() {
        use std::collections::HashMap;

        let adapter = V2025_11_25Adapter;

        let mut extensions = HashMap::new();
        extensions.insert(
            "io.modelcontextprotocol/trace".to_string(),
            serde_json::json!({"version": "1"}),
        );
        let caps = ServerCapabilities {
            extensions: Some(extensions),
            ..Default::default()
        };
        let filtered = adapter.filter_capabilities(caps);
        assert!(
            filtered.extensions.is_none(),
            "extensions field should be stripped for stable 2025-11-25"
        );

        let result = json!({
            "capabilities": {
                "tools": { "listChanged": true },
                "extensions": { "io.modelcontextprotocol/trace": { "version": "1" } }
            }
        });
        let filtered = adapter.filter_result("initialize", result);
        assert!(filtered["capabilities"]["tools"].is_object());
        assert!(
            filtered["capabilities"].get("extensions").is_none(),
            "extensions key should be stripped from initialize result"
        );
    }

    #[test]
    fn test_draft_preserves_extensions() {
        use std::collections::HashMap;

        let adapter = DraftAdapter;

        let mut extensions = HashMap::new();
        extensions.insert(
            "io.modelcontextprotocol/trace".to_string(),
            serde_json::json!({"version": "1"}),
        );
        let caps = ServerCapabilities {
            extensions: Some(extensions),
            ..Default::default()
        };
        let filtered = adapter.filter_capabilities(caps);
        assert!(
            filtered
                .extensions
                .as_ref()
                .is_some_and(|m| m.contains_key("io.modelcontextprotocol/trace")),
            "draft adapter must preserve extensions"
        );
    }

    #[test]
    fn test_v2025_06_18_strips_tools_icons() {
        let adapter = V2025_06_18Adapter;
        let result = json!({
            "tools": [
                {
                    "name": "my-tool",
                    "description": "A tool",
                    "inputSchema": { "type": "object" },
                    "icons": [{ "src": "https://example.com/icon.png" }],
                    "execution": { "taskSupport": "optional" },
                    "outputSchema": { "type": "object" }
                }
            ]
        });

        let filtered = adapter.filter_result("tools/list", result);
        let tool = &filtered["tools"][0];
        assert!(tool.get("name").is_some());
        assert!(tool.get("description").is_some());
        assert!(tool.get("icons").is_none(), "icons should be stripped");
        assert!(
            tool.get("execution").is_none(),
            "execution should be stripped"
        );
        assert!(
            tool.get("outputSchema").is_none(),
            "outputSchema should be stripped"
        );
    }

    #[test]
    fn test_v2025_06_18_strips_server_info() {
        let adapter = V2025_06_18Adapter;
        let result = json!({
            "protocolVersion": "2025-06-18",
            "serverInfo": {
                "name": "my-server",
                "version": "1.0.0",
                "description": "A server",
                "icons": [{ "src": "https://example.com/icon.png" }],
                "websiteUrl": "https://example.com"
            },
            "capabilities": {
                "tools": { "listChanged": true },
                "tasks": { "list": {} }
            }
        });

        let filtered = adapter.filter_result("initialize", result);
        let info = &filtered["serverInfo"];
        assert!(info.get("name").is_some());
        assert!(
            info.get("description").is_none(),
            "description should be stripped"
        );
        assert!(info.get("icons").is_none(), "icons should be stripped");
        assert!(
            info.get("websiteUrl").is_none(),
            "websiteUrl should be stripped"
        );

        let caps = &filtered["capabilities"];
        assert!(caps.get("tools").is_some());
        assert!(
            caps.get("tasks").is_none(),
            "tasks capability should be stripped"
        );
    }

    #[test]
    fn test_v2025_06_18_rejects_task_methods() {
        let adapter = V2025_06_18Adapter;
        assert!(adapter.validate_method("tools/list").is_ok());
        assert!(adapter.validate_method("tools/call").is_ok());
        assert!(adapter.validate_method("tasks/get").is_err());
        assert!(adapter.validate_method("tasks/list").is_err());
        assert!(
            adapter
                .validate_method("notifications/tasks/status")
                .is_err()
        );
    }

    #[test]
    fn test_v2025_06_18_strips_prompts_icons() {
        let adapter = V2025_06_18Adapter;
        let result = json!({
            "prompts": [
                {
                    "name": "my-prompt",
                    "description": "A prompt",
                    "icons": [{ "src": "https://example.com/icon.png" }]
                }
            ]
        });

        let filtered = adapter.filter_result("prompts/list", result);
        let prompt = &filtered["prompts"][0];
        assert!(prompt.get("name").is_some());
        assert!(prompt.get("icons").is_none(), "icons should be stripped");
    }

    #[test]
    fn test_v2025_06_18_strips_resources_icons() {
        let adapter = V2025_06_18Adapter;
        let result = json!({
            "resources": [
                {
                    "uri": "file:///test.txt",
                    "name": "test",
                    "icons": [{ "src": "https://example.com/icon.png" }]
                }
            ]
        });

        let filtered = adapter.filter_result("resources/list", result);
        let resource = &filtered["resources"][0];
        assert!(resource.get("uri").is_some());
        assert!(resource.get("icons").is_none(), "icons should be stripped");
    }

    #[test]
    fn test_draft_passthrough() {
        let adapter = DraftAdapter;
        assert!(adapter.validate_method("tools/list").is_ok());
        assert!(adapter.validate_method("tasks/get").is_ok());
    }

    #[test]
    fn test_method_sets_are_consistent() {
        // 2025-11-25 should be a superset of 2025-06-18
        for method in METHODS_2025_06_18.iter() {
            assert!(
                METHODS_2025_11_25.contains(method),
                "2025-11-25 should contain all 2025-06-18 methods, missing: {method}"
            );
        }

        // 2025-11-25 only should have no overlap with 2025-06-18
        for method in METHODS_2025_11_25_ONLY.iter() {
            assert!(
                !METHODS_2025_06_18.contains(method),
                "2025-11-25-only method {method} should not be in 2025-06-18"
            );
        }
    }

    #[test]
    fn test_elicitation_capabilities_backward_compat() {
        use crate::types::capabilities::ElicitationCapabilities;

        // Empty object should support form mode (backward compat)
        let empty = ElicitationCapabilities::default();
        assert!(
            empty.supports_form(),
            "empty caps should default to form support"
        );
        assert!(
            !empty.supports_url(),
            "empty caps should not support URL mode"
        );

        // Explicit form+url
        let full = ElicitationCapabilities::full();
        assert!(full.supports_form());
        assert!(full.supports_url());

        // Form only
        let form = ElicitationCapabilities::form_only();
        assert!(form.supports_form());
        assert!(!form.supports_url());
    }
}
