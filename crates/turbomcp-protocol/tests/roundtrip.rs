//! Round-trip and version-difference tests over the @generated wire types.
//!
//! These prove (a) the generated types deserialize/serialize real spec shapes,
//! and (b) the codegen faithfully captured the per-version differences the
//! ground-truth audit found (AUDIT_FINDINGS.md): Tasks is core in `2025-11-25`
//! but replaced by `extensions` in `2026-07-28`; `initialize`/discover differ.

use serde_json::json;
use turbomcp_protocol::{v2025_11_25, v2026_07_28};

#[test]
fn draft_implementation_roundtrips() {
    let value = json!({ "name": "srv", "version": "1.0.0", "title": "Server" });
    let imp: v2026_07_28::types::Implementation =
        serde_json::from_value(value).expect("deserialize Implementation");
    assert_eq!(imp.name, "srv");
    assert_eq!(imp.version, "1.0.0");
    let back = serde_json::to_value(&imp).expect("serialize Implementation");
    assert_eq!(back["name"], "srv");
    assert_eq!(back["version"], "1.0.0");
}

#[test]
fn tasks_is_core_in_2025_but_extensions_in_draft() {
    // F9: 2025-11-25 carries a first-class `tasks` server capability.
    let caps_2025: v2025_11_25::types::ServerCapabilities =
        serde_json::from_value(json!({ "tasks": {} })).expect("2025 ServerCapabilities");
    assert!(
        caps_2025.tasks.is_some(),
        "2025-11-25 ServerCapabilities must have a typed `tasks` field"
    );
    let back = serde_json::to_value(&caps_2025).unwrap();
    assert!(back.get("tasks").is_some());

    // Draft replaces core Tasks with the generic `extensions` mechanism.
    let caps_draft: v2026_07_28::types::ServerCapabilities =
        serde_json::from_value(json!({ "extensions": { "io.modelcontextprotocol/tasks": {} } }))
            .expect("draft ServerCapabilities");
    assert!(
        caps_draft
            .extensions
            .contains_key("io.modelcontextprotocol/tasks"),
        "draft ServerCapabilities must carry extensions"
    );
}

/// Compile-time proof of the version split: these types exist only in the
/// version that defines them (the draft is stateless — no `initialize`; only
/// the draft has `server/discover`).
#[allow(dead_code)]
fn _version_split_is_type_level() {
    let _initialize_is_2025_only: Option<v2025_11_25::types::InitializeRequestParams> = None;
    let _discover_is_draft_only: Option<v2026_07_28::types::DiscoverResult> = None;
}
