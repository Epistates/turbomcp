//! The neutral types a client reads: server capabilities as any revision
//! sends them, progress updates, and the consumer helpers on a tool result.

use serde_json::json;
use turbomcp_protocol::neutral::{CallToolResult, Content, Progress, ServerCapabilities};

#[test]
fn capabilities_read_the_same_from_every_revision() {
    let stateful = json!({
        "tools": { "listChanged": true },
        "resources": { "subscribe": true },
        "logging": {},
        "tasks": { "list": {} },
    });
    let caps = ServerCapabilities::from_wire(&stateful);
    assert!(caps.tools.unwrap().list_changed);
    let resources = caps.resources.unwrap();
    assert!(resources.subscribe && !resources.list_changed);
    assert!(caps.prompts.is_none());
    assert!(caps.logging && !caps.completions);
    assert_eq!(caps.tasks, Some(json!({ "list": {} })));

    let stateless = json!({
        "prompts": {},
        "completions": {},
        "extensions": { "io.modelcontextprotocol/tasks": {} },
    });
    let caps = ServerCapabilities::from_wire(&stateless);
    assert!(!caps.prompts.unwrap().list_changed);
    assert!(caps.completions);
    assert!(
        caps.extensions
            .contains_key("io.modelcontextprotocol/tasks")
    );
    assert!(caps.tools.is_none());
}

/// A member that isn't an object isn't a declaration.
#[test]
fn malformed_members_read_as_undeclared() {
    let caps = ServerCapabilities::from_wire(&json!({ "tools": true, "logging": null }));
    assert!(caps.tools.is_none());
    assert!(!caps.logging);
}

#[test]
fn progress_reads_from_notification_params() {
    let p =
        Progress::from_params(&json!({ "progressToken": 1, "progress": 3, "total": 4 })).unwrap();
    assert_eq!(p.fraction(), Some(0.75));
    assert!(p.message.is_none());
    assert!(Progress::from_params(&json!({ "progressToken": 1 })).is_none());
}

#[test]
fn tool_result_helpers() {
    let mut result = CallToolResult::new(vec![Content::text("a"), Content::text("b")]);
    result.structured_content = Some(json!({ "n": 3 }));
    assert_eq!(result.text_content().as_deref(), Some("a\nb"));

    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct Out {
        n: u32,
    }
    assert_eq!(result.structured::<Out>().unwrap(), Some(Out { n: 3 }));
    assert!(result.clone().into_result().is_ok());

    let failed = CallToolResult::error("quota");
    assert!(failed.structured::<Out>().unwrap().is_none());
    assert_eq!(
        failed.into_result().unwrap_err().text_content().as_deref(),
        Some("quota")
    );
}
