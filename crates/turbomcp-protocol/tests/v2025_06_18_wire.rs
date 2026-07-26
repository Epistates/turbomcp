//! The `2025-06-18` wire: what it carries, and — more importantly — what it
//! must not.
//!
//! `2025-06-18` is `2025-11-25` minus Tasks, `icons`, and two `Implementation`
//! fields. The conversions step down from the `2025-11-25` wire rather than
//! being written a third time (see `v2025_06_18::convert`), so the risk these
//! tests exist for is a `2025-11-25` addition leaking onto a wire whose clients
//! have never heard of it — and the mirror risk of the step-down quietly taking
//! something that *is* shared along with it.

use serde_json::{Value, json};
use turbomcp_protocol::neutral;
use turbomcp_protocol::v2025_06_18::types as v06;

fn wire<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

/// A tool using every field `2025-11-25` added on top of `2025-06-18`.
fn maximal_tool() -> neutral::Tool {
    neutral::Tool::new("search", json!({ "type": "object", "properties": {} }))
        .with_title("Search")
        .with_description("Find things")
        .with_task_support(neutral::TaskSupport::Optional)
        .with_annotations(neutral::ToolAnnotations::new().read_only())
        .with_icon(neutral::Icon::new("https://example.com/i.png"))
        .with_meta_entry("io.example/tag", json!("catalog"))
}

/// The additions must not appear. A `2025-06-18` client has no `icons` and no
/// Tasks; sending either is a field it will not understand, and `execution`
/// in particular advertises a capability the server will refuse on that wire.
#[test]
fn a_tool_sheds_every_field_the_revision_does_not_have() {
    let v: v06::Tool = maximal_tool().into();
    let v = wire(&v);

    assert!(
        v.get("icons").is_none(),
        "icons is a 2025-11-25 addition: {v}"
    );
    assert!(
        v.get("execution").is_none(),
        "task support has no meaning without Tasks: {v}"
    );

    // Everything the revision *does* have is still there.
    assert_eq!(v["name"], "search");
    assert_eq!(v["title"], "Search");
    assert_eq!(v["description"], "Find things");
    assert_eq!(v["annotations"]["readOnlyHint"], true);
    assert_eq!(v["_meta"]["io.example/tag"], "catalog");
    assert_eq!(v["inputSchema"]["type"], "object");
}

/// The same for the other four types that gained `icons`.
#[test]
fn icons_are_dropped_from_every_type_that_carries_them() {
    let icon = || neutral::Icon::new("https://example.com/i.png");

    let resource: v06::Resource = neutral::Resource::new("mem://a", "a")
        .with_icon(icon())
        .with_title("A")
        .into();
    let v = wire(&resource);
    assert!(v.get("icons").is_none(), "{v}");
    assert_eq!(v["title"], "A", "the rest survives: {v}");

    let template: v06::ResourceTemplate = neutral::ResourceTemplate::new("mem://{id}", "t")
        .with_icon(icon())
        .into();
    assert!(wire(&template).get("icons").is_none());

    let prompt: v06::Prompt = neutral::Prompt::new("p").with_icon(icon()).into();
    assert!(wire(&prompt).get("icons").is_none());

    // ResourceLink rides inside a tool result's content.
    let result: v06::CallToolResult =
        neutral::CallToolResult::new(vec![neutral::Content::resource_link(
            neutral::Resource::new("mem://b", "b").with_icon(icon()),
        )])
        .into();
    let v = wire(&result);
    assert!(v["content"][0].get("icons").is_none(), "{v}");
    assert_eq!(v["content"][0]["type"], "resource_link");
}

/// A tool schema that uses JSON Schema keywords the wire struct doesn't name —
/// `$defs`, `additionalProperties`, `$schema` — must survive intact.
///
/// This is the case that made the codegen's schema-opening heuristic wrong for
/// this revision: `2025-06-18` describes `inputSchema` without a `$schema`
/// property, so the node was left closed and every unnamed keyword was dropped
/// on serialization. Any tool taking a nested type would have advertised a
/// `$ref` into a `$defs` that no longer existed — a schema no client could
/// resolve.
#[test]
fn an_input_schema_keeps_the_keywords_the_struct_does_not_name() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": { "q": { "$ref": "#/$defs/Query" } },
        "required": ["q"],
        "additionalProperties": false,
        "$defs": { "Query": { "type": "string", "minLength": 1 } },
    });
    let tool: v06::Tool = neutral::Tool::new("search", schema).into();
    let v = wire(&tool);
    let emitted = &v["inputSchema"];

    assert_eq!(
        emitted["$defs"]["Query"]["type"], "string",
        "a dropped $defs leaves the $ref below dangling: {emitted}"
    );
    assert_eq!(emitted["properties"]["q"]["$ref"], "#/$defs/Query");
    assert_eq!(emitted["additionalProperties"], false);
    assert_eq!(
        emitted["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "`$schema` has no declared field here, so it must ride in the open part"
    );
    assert_eq!(emitted["required"], json!(["q"]));
}

/// Round trip: a client reading a `2025-06-18` response gets back everything
/// the revision can express. The dropped fields come back at their defaults,
/// not as garbage.
#[test]
fn a_tool_round_trips_through_the_wire_and_back() {
    let wire_tool: v06::Tool = maximal_tool().into();
    let back: neutral::Tool = wire_tool.into();

    assert_eq!(back.name, "search");
    assert_eq!(back.title.as_deref(), Some("Search"));
    assert_eq!(back.description.as_deref(), Some("Find things"));
    assert_eq!(back.annotations.and_then(|a| a.read_only_hint), Some(true));
    assert_eq!(back.input_schema["type"], "object");
    assert!(
        back.icons.is_empty(),
        "the wire carried none, so none come back"
    );
    assert_eq!(
        back.task_support, None,
        "Tasks do not exist on this revision"
    );
}

/// Content blocks, resource contents, and prompt messages are shared verbatim
/// between the two revisions — the step-down must not lose them.
#[test]
fn the_shared_surface_survives_the_step_down() {
    let result: v06::CallToolResult = neutral::CallToolResult::new(vec![
        neutral::Content::text("hello"),
        neutral::Content::image("aGk=", "image/png"),
        neutral::Content::audio("aGk=", "audio/wav"),
    ])
    .into();
    let v = wire(&result);
    assert_eq!(v["content"][0]["text"], "hello");
    assert_eq!(v["content"][1]["mimeType"], "image/png");
    assert_eq!(v["content"][2]["type"], "audio");
    assert_eq!(v["isError"], false);

    let read: v06::ReadResourceResult = neutral::ReadResourceResult::text("mem://a", "body").into();
    let v = wire(&read);
    assert_eq!(v["contents"][0]["uri"], "mem://a");
    assert_eq!(v["contents"][0]["text"], "body");

    let prompt: v06::GetPromptResult = neutral::GetPromptResult::new(vec![
        neutral::PromptMessage::user_text("ask"),
        neutral::PromptMessage::assistant_text("answer"),
    ])
    .into();
    let v = wire(&prompt);
    assert_eq!(v["messages"][0]["role"], "user");
    assert_eq!(v["messages"][0]["content"]["text"], "ask");
    assert_eq!(v["messages"][1]["role"], "assistant");

    let complete: v06::CompleteResult =
        neutral::CompleteResult::new(vec!["a".into(), "b".into()]).into();
    assert_eq!(wire(&complete)["completion"]["values"], json!(["a", "b"]));
}

/// The draft's cacheability envelope (`resultType`/`ttlMs`/`cacheScope`) is
/// draft-only and must not appear on this wire, exactly as it must not on
/// `2025-11-25`.
#[test]
fn the_draft_cache_envelope_never_appears() {
    let result: v06::ListToolsResult = neutral::ListToolsResult::new(vec![])
        .with_cache(neutral::CachePolicy::public(
            core::time::Duration::from_secs(60),
        ))
        .into();
    let v = wire(&result);
    for key in ["resultType", "ttlMs", "cacheScope"] {
        assert!(v.get(key).is_none(), "{key} is draft-only: {v}");
    }
}
