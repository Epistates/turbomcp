//! The official schema as the test oracle.
//!
//! Every other conversion test compares typed values in memory, which is how
//! four wire bugs survived them: typify had dropped the `type` discriminators
//! (audio decoded as image) and could not tell `{}` from absent (a client's
//! `elicitation.url` marker and an empty `structuredContent` vanished), and a
//! boolean property subschema wiped a whole tool schema. None of those is
//! visible without going through bytes and the spec's own `schema.json`.
//!
//! So each case here renders neutral values onto every wire, serializes them,
//! validates the bytes against that revision's schema definition, decodes them
//! back, and checks nothing moved. The schemas and the `2026-07-28` examples
//! are vendored from the same upstream tag as the generated types by
//! `just codegen`, and drift-checked with them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use turbomcp_protocol::neutral;
use turbomcp_protocol::v2025_06_18::types as v0618;
use turbomcp_protocol::v2025_11_25::types as v1125;
use turbomcp_protocol::v2026_07_28::types as v0728;

fn fixtures(rev: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/schema/{rev}"))
}

struct Oracle {
    rev: &'static str,
    root: Value,
    defs_key: &'static str,
}

impl Oracle {
    fn load(rev: &'static str) -> Self {
        let path = fixtures(rev).join("schema.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run `just codegen`)", path.display()));
        let root: Value = serde_json::from_str(&raw).expect("schema.json is JSON");
        let defs_key = if root.get("$defs").is_some() {
            "$defs"
        } else {
            "definitions"
        };
        Self {
            rev,
            root,
            defs_key,
        }
    }

    /// Validate `instance` against the named definition.
    fn validate(&self, def: &str, instance: &Value) {
        assert!(
            self.root[self.defs_key].get(def).is_some(),
            "{}: no definition named {def}",
            self.rev
        );
        let mut schema = self.root.clone();
        schema["$ref"] = Value::String(format!("#/{}/{def}", self.defs_key));
        let validator = jsonschema::validator_for(&schema)
            .unwrap_or_else(|e| panic!("{}: compiling {def}: {e}", self.rev));
        let errors: Vec<String> = validator
            .iter_errors(instance)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{} {def} violates the schema:\n  {}\ninstance: {instance:#}",
            self.rev,
            errors.join("\n  ")
        );
    }

    /// `doc` without the top-level keys `def` neither declares nor opens up
    /// with `additionalProperties`. A typed struct drops those, and at least one
    /// upstream example (`ListRootsRequest`, with an `id`) carries one.
    fn declared_only(&self, def: &str, doc: &Value) -> Value {
        let schema = &self.root[self.defs_key][def];
        let (Some(props), Some(fields)) = (schema["properties"].as_object(), doc.as_object())
        else {
            return doc.clone();
        };
        if schema.get("additionalProperties").is_some() {
            return doc.clone();
        }
        Value::Object(
            fields
                .iter()
                .filter(|(k, _)| props.contains_key(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }

    /// Serialize, validate, decode from the bytes, and check the decode
    /// re-serializes to the same document. Returns the decoded value and the
    /// JSON that went over the wire.
    fn wire<W: Serialize + DeserializeOwned>(&self, def: &str, wire: &W) -> (W, Value) {
        let bytes = serde_json::to_vec(wire).expect("serialize");
        let sent: Value = serde_json::from_slice(&bytes).expect("bytes are JSON");
        self.validate(def, &sent);
        let back: W = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("{} {def} does not decode its own output: {e}", self.rev));
        assert_eq!(
            serde_json::to_value(&back).expect("re-serialize"),
            sent,
            "{} {def} changed across a decode",
            self.rev
        );
        (back, sent)
    }
}

struct Wires {
    v0618: Oracle,
    v1125: Oracle,
    v0728: Oracle,
}

fn wires() -> Wires {
    Wires {
        v0618: Oracle::load("2025-06-18"),
        v1125: Oracle::load("2025-11-25"),
        v0728: Oracle::load("2026-07-28"),
    }
}

/// One neutral value through all three wires and back.
macro_rules! through_every_wire {
    ($w:expr, $def:literal, $neutral:ty, $wire:ident, $value:expr) => {{
        let value: $neutral = $value;
        let (v0728_back, v0728_sent) = $w.v0728.wire($def, &v0728::$wire::from(value.clone()));
        let (v1125_back, v1125_sent) = $w.v1125.wire($def, &v1125::$wire::from(value.clone()));
        let (v0618_back, v0618_sent) = $w
            .v0618
            .wire($def, &v0618::$wire::from(v1125::$wire::from(value.clone())));
        let from_v0728: $neutral = v0728_back.into();
        let from_v1125: $neutral = v1125_back.into();
        let from_v0618: $neutral = v1125::$wire::from(v0618_back).into();
        (
            [from_v0728, from_v1125, from_v0618],
            [v0728_sent, v1125_sent, v0618_sent],
        )
    }};
}

fn every_content_variant() -> Vec<neutral::Content> {
    vec![
        neutral::Content::text("hello"),
        neutral::Content::image("aW1n", "image/png"),
        neutral::Content::audio("YXVk", "audio/wav"),
        neutral::Content::resource(neutral::ResourceContents::text("file:///a.txt", "body")),
        neutral::Content::resource_link(
            neutral::Resource::new("file:///b.txt", "b").with_mime_type("text/plain"),
        ),
    ]
}

#[test]
fn every_content_variant_survives_every_wire() {
    let w = wires();
    let content = every_content_variant();
    let (decoded, sent) = through_every_wire!(
        w,
        "CallToolResult",
        neutral::CallToolResult,
        CallToolResult,
        neutral::CallToolResult::new(content.clone())
    );
    for (i, result) in decoded.iter().enumerate() {
        assert_eq!(result.content, content, "wire #{i} moved a content block");
    }
    // The discriminator is what an untagged decode used to ignore.
    for doc in &sent {
        assert_eq!(doc["content"][2]["type"], "audio", "{doc:#}");
    }
}

/// `2025-06-18`/`2025-11-25` typed `structuredContent` as a map that serde
/// skipped when empty; a tool whose output was `{}` then sent no
/// `structuredContent` next to its advertised `outputSchema`, which python-sdk's
/// client treats as an error.
#[test]
fn an_empty_structured_content_object_is_still_sent() {
    let w = wires();
    let mut result = neutral::CallToolResult::text("{}");
    result.structured_content = Some(json!({}));
    let (decoded, sent) = through_every_wire!(
        w,
        "CallToolResult",
        neutral::CallToolResult,
        CallToolResult,
        result
    );
    for doc in &sent {
        assert_eq!(doc.get("structuredContent"), Some(&json!({})), "{doc:#}");
    }
    for back in decoded {
        assert_eq!(back.structured_content, Some(json!({})));
    }
}

/// schemars emits `true` for a `serde_json::Value` field. The legacy wires
/// typed each property schema as an object, so the whole `inputSchema` failed
/// to parse: a server advertised a bare `{"type":"object"}` and a client's
/// `tools/list` failed outright. Their schemas also make `true` invalid there,
/// so it goes out as the equivalent `{}`; `2026-07-28` carries it as written.
#[test]
fn boolean_property_subschemas_and_empty_properties_survive() {
    let w = wires();
    let schema = json!({
        "type": "object",
        "properties": { "key": { "type": "string" }, "value": true, "never": false },
        "required": ["key", "value"],
        "additionalProperties": false
    });
    let spelled = json!({
        "type": "object",
        "properties": { "key": { "type": "string" }, "value": {}, "never": { "not": {} } },
        "required": ["key", "value"],
        "additionalProperties": false
    });
    let tools = neutral::ListToolsResult::new(vec![
        neutral::Tool::new("put", schema.clone()),
        neutral::Tool::new("ping", json!({ "type": "object", "properties": {} })),
    ]);
    let (decoded, sent) = through_every_wire!(
        w,
        "ListToolsResult",
        neutral::ListToolsResult,
        ListToolsResult,
        tools
    );
    let [v0728_doc, v1125_doc, v0618_doc] = &sent;
    assert_eq!(v0728_doc["tools"][0]["inputSchema"], schema);
    assert_eq!(v1125_doc["tools"][0]["inputSchema"], spelled);
    assert_eq!(v0618_doc["tools"][0]["inputSchema"], spelled);
    for doc in &sent {
        assert_eq!(
            doc["tools"][1]["inputSchema"]["properties"],
            json!({}),
            "{doc:#}"
        );
    }
    let [from_v0728, from_v1125, from_v0618] = &decoded;
    assert_eq!(from_v0728.tools[0].input_schema, schema);
    assert_eq!(from_v1125.tools[0].input_schema, spelled);
    assert_eq!(from_v0618.tools[0].input_schema, spelled);

    // And what an rmcp server actually sends still decodes.
    let from_rmcp: v1125::Tool = serde_json::from_value(json!({
        "name": "put",
        "inputSchema": { "type": "object", "properties": { "value": true } }
    }))
    .expect("a boolean property subschema decodes");
    let tool: neutral::Tool = from_rmcp.into();
    assert_eq!(tool.input_schema["properties"]["value"], json!(true));
}

/// A capability sub-object is a presence marker. `{"elicitation":{"url":{}}}`
/// used to re-serialize as `{"elicitation":{}}`, which means "form only": a
/// URL-only client was then sent forms.
#[test]
fn capability_presence_markers_survive_a_typed_round_trip() {
    let w = wires();
    let declared = json!({
        "elicitation": { "url": {} },
        "sampling": { "tools": {} },
        "tasks": { "requests": { "sampling": { "createMessage": {} } } }
    });
    let caps: v1125::ClientCapabilities = serde_json::from_value(declared.clone()).unwrap();
    let (_, sent) = w.v1125.wire("ClientCapabilities", &caps);
    assert_eq!(sent, declared);

    let server = json!({ "completions": {}, "logging": {}, "tasks": { "list": {}, "cancel": {} } });
    let caps: v1125::ServerCapabilities = serde_json::from_value(server.clone()).unwrap();
    let (_, sent) = w.v1125.wire("ServerCapabilities", &caps);
    assert_eq!(sent, server);
}

/// `2026-07-28`'s `JSONValue` admits only strings, integers and booleans, so a
/// float or a `null` in a peer's extension settings failed the whole typed
/// decode. The schema really does exclude them, so this is decode leniency,
/// not something the oracle would pass on the way out.
#[test]
fn extension_settings_from_a_peer_may_hold_any_json() {
    let caps = json!({
        "extensions": { "io.example/x": { "ratio": 0.5, "unset": null, "list": [1.5, null] } }
    });
    let parsed: v0728::ServerCapabilities = serde_json::from_value(caps.clone())
        .expect("floats and nulls in extension settings decode");
    assert_eq!(serde_json::to_value(&parsed).unwrap(), caps);
}

/// "clients **MUST** treat an absent `resultType` as `"complete"`."
#[test]
fn an_absent_result_type_decodes_as_complete() {
    let result: v0728::ListToolsResult = serde_json::from_value(json!({
        "tools": [], "ttlMs": 0, "cacheScope": "private"
    }))
    .expect("an absent resultType decodes");
    assert_eq!(result.result_type, "complete");
}

/// The protocol-level unions are untagged too. Without the method pinned, every
/// request decoded as whichever variant came first and fit.
#[test]
fn protocol_unions_select_by_method_and_type() {
    let request: v1125::ClientRequest = serde_json::from_value(json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/list"
    }))
    .unwrap();
    assert!(
        matches!(request, v1125::ClientRequest::ListToolsRequest(_)),
        "{request:?}"
    );

    let notification: v1125::ServerNotification = serde_json::from_value(json!({
        "jsonrpc": "2.0", "method": "notifications/tools/list_changed"
    }))
    .unwrap();
    assert!(
        matches!(
            notification,
            v1125::ServerNotification::ToolListChangedNotification(_)
        ),
        "{notification:?}"
    );

    let audio: v0618::ContentBlock = serde_json::from_value(json!({
        "type": "audio", "data": "YXVk", "mimeType": "audio/wav"
    }))
    .unwrap();
    assert!(
        matches!(audio, v0618::ContentBlock::AudioContent(_)),
        "{audio:?}"
    );
}

/// Replay every example the `2026-07-28` schema ships: each must validate
/// (which checks this harness), decode into its generated type, and
/// re-serialize to the same document.
#[test]
fn every_2026_07_28_example_decodes_and_round_trips() {
    let oracle = Oracle::load("2026-07-28");
    let examples = fixtures("2026-07-28").join("examples");
    let mut seen = BTreeMap::new();

    macro_rules! replay {
        ($(($dir:literal, $ty:ident)),* $(,)?) => {$(
            let dir = examples.join($dir);
            let mut files: Vec<_> = std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
                .map(|entry| entry.unwrap().path())
                .collect();
            files.sort();
            for file in &files {
                let raw = std::fs::read_to_string(file).unwrap();
                let doc: Value = serde_json::from_str(&raw).unwrap();
                oracle.validate($dir, &doc);
                let typed: v0728::$ty = serde_json::from_value(doc.clone())
                    .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
                let back = without_decode_defaults(serde_json::to_value(&typed).unwrap(), &doc);
                let expected = oracle.declared_only($dir, &doc);
                assert!(
                    same_json(&back, &expected),
                    "{} changed across a typed round trip\n  sent: {expected}\n  back: {back}",
                    file.display()
                );
            }
            seen.insert($dir, files.len());
        )*};
    }

    replay!(
        ("AudioContent", AudioContent),
        ("BlobResourceContents", BlobResourceContents),
        ("BooleanSchema", BooleanSchema),
        ("CallToolRequest", CallToolRequest),
        ("CallToolRequestParams", CallToolRequestParams),
        ("CallToolResult", CallToolResult),
        ("CallToolResultResponse", CallToolResultResponse),
        ("CancelledNotification", CancelledNotification),
        ("CancelledNotificationParams", CancelledNotificationParams),
        ("ClientCapabilities", ClientCapabilities),
        ("CompleteRequest", CompleteRequest),
        ("CompleteRequestParams", CompleteRequestParams),
        ("CompleteResult", CompleteResult),
        ("CompleteResultResponse", CompleteResultResponse),
        ("CreateMessageRequest", CreateMessageRequest),
        ("CreateMessageRequestParams", CreateMessageRequestParams),
        ("CreateMessageResult", CreateMessageResult),
        ("DiscoverRequest", DiscoverRequest),
        ("DiscoverResult", DiscoverResult),
        ("DiscoverResultResponse", DiscoverResultResponse),
        ("ElicitRequest", ElicitRequest),
        ("ElicitRequestFormParams", ElicitRequestFormParams),
        ("ElicitRequestURLParams", ElicitRequestUrlParams),
        ("ElicitResult", ElicitResult),
        ("EmbeddedResource", EmbeddedResource),
        ("GetPromptRequest", GetPromptRequest),
        ("GetPromptRequestParams", GetPromptRequestParams),
        ("GetPromptResult", GetPromptResult),
        ("GetPromptResultResponse", GetPromptResultResponse),
        ("HeaderMismatchError", HeaderMismatchError),
        ("ImageContent", ImageContent),
        ("InputRequests", InputRequests),
        ("InputRequiredResult", InputRequiredResult),
        ("InputResponses", InputResponses),
        ("InternalError", InternalError),
        ("InvalidParamsError", InvalidParamsError),
        ("ListPromptsRequest", ListPromptsRequest),
        ("ListPromptsResult", ListPromptsResult),
        ("ListPromptsResultResponse", ListPromptsResultResponse),
        ("ListResourcesRequest", ListResourcesRequest),
        ("ListResourcesResult", ListResourcesResult),
        ("ListResourcesResultResponse", ListResourcesResultResponse),
        ("ListResourceTemplatesRequest", ListResourceTemplatesRequest),
        ("ListResourceTemplatesResult", ListResourceTemplatesResult),
        (
            "ListResourceTemplatesResultResponse",
            ListResourceTemplatesResultResponse
        ),
        ("ListRootsRequest", ListRootsRequest),
        ("ListRootsResult", ListRootsResult),
        ("ListToolsRequest", ListToolsRequest),
        ("ListToolsResult", ListToolsResult),
        ("ListToolsResultResponse", ListToolsResultResponse),
        ("LoggingMessageNotification", LoggingMessageNotification),
        (
            "LoggingMessageNotificationParams",
            LoggingMessageNotificationParams
        ),
        ("MethodNotFoundError", MethodNotFoundError),
        (
            "MissingRequiredClientCapabilityError",
            MissingRequiredClientCapabilityError
        ),
        ("ModelPreferences", ModelPreferences),
        ("NumberSchema", NumberSchema),
        ("PaginatedRequestParams", PaginatedRequestParams),
        ("ParseError", ParseError),
        ("ProgressNotification", ProgressNotification),
        ("ProgressNotificationParams", ProgressNotificationParams),
        (
            "PromptListChangedNotification",
            PromptListChangedNotification
        ),
        ("ReadResourceRequest", ReadResourceRequest),
        ("ReadResourceResult", ReadResourceResult),
        ("ReadResourceResultResponse", ReadResourceResultResponse),
        ("Resource", Resource),
        ("ResourceLink", ResourceLink),
        (
            "ResourceListChangedNotification",
            ResourceListChangedNotification
        ),
        ("ResourceUpdatedNotification", ResourceUpdatedNotification),
        (
            "ResourceUpdatedNotificationParams",
            ResourceUpdatedNotificationParams
        ),
        ("Root", Root),
        ("SamplingMessage", SamplingMessage),
        ("ServerCapabilities", ServerCapabilities),
        ("StringSchema", StringSchema),
        (
            "SubscriptionsAcknowledgedNotification",
            SubscriptionsAcknowledgedNotification
        ),
        ("SubscriptionsListenRequest", SubscriptionsListenRequest),
        ("SubscriptionsListenResult", SubscriptionsListenResult),
        (
            "SubscriptionsListenResultResponse",
            SubscriptionsListenResultResponse
        ),
        ("TextContent", TextContent),
        ("TextResourceContents", TextResourceContents),
        ("TitledMultiSelectEnumSchema", TitledMultiSelectEnumSchema),
        ("TitledSingleSelectEnumSchema", TitledSingleSelectEnumSchema),
        ("Tool", Tool),
        ("ToolListChangedNotification", ToolListChangedNotification),
        ("ToolResultContent", ToolResultContent),
        ("ToolUseContent", ToolUseContent),
        (
            "UnsupportedProtocolVersionError",
            UnsupportedProtocolVersionError
        ),
        (
            "UntitledMultiSelectEnumSchema",
            UntitledMultiSelectEnumSchema
        ),
        (
            "UntitledSingleSelectEnumSchema",
            UntitledSingleSelectEnumSchema
        ),
    );

    // A directory upstream adds is a type this list has never seen.
    let on_disk = std::fs::read_dir(&examples)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().is_dir())
        .count();
    assert_eq!(seen.len(), on_disk, "an examples directory is not replayed");
}

/// JSON equality: `50` and `50.0` are the same number, which `Value`'s own
/// `PartialEq` does not agree with once a field is typed `f64`.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_json(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w)))
        }
        _ => a == b,
    }
}

/// `back` without the keys decoding filled in: those the sender left out
/// whose value is the documented default (`resultType: "complete"`,
/// `ttlMs: 0`, `cacheScope: "private"`).
fn without_decode_defaults(mut back: Value, sent: &Value) -> Value {
    let defaults = [
        ("resultType", json!("complete")),
        ("ttlMs", json!(0)),
        ("cacheScope", json!("private")),
    ];
    match (&mut back, sent) {
        (Value::Object(fields), Value::Object(original)) => {
            for (key, default) in &defaults {
                if !original.contains_key(*key) && fields.get(*key) == Some(default) {
                    fields.remove(*key);
                }
            }
            for (key, value) in fields.iter_mut() {
                if let Some(original) = original.get(key) {
                    *value = without_decode_defaults(value.take(), original);
                }
            }
        }
        (Value::Array(items), Value::Array(original)) => {
            for (item, original) in items.iter_mut().zip(original) {
                *item = without_decode_defaults(item.take(), original);
            }
        }
        _ => {}
    }
    back
}
