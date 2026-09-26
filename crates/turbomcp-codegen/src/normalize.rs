//! Schema normalization passes, applied in order by `main` before typify runs:
//!
//! 1. [`flatten_all_of`] – typify cannot merge the task result intersections.
//! 2. [`open_embedded_schemas`] – keep tool/elicitation schemas open.
//! 3. [`open_meta_objects`] – keep `_meta` shapes open.
//! 4. [`allow_boolean_subschemas`] – `properties: {"x": true}` is legal.
//! 5. [`pin_string_consts`] – make discriminators discriminate.
//! 6. [`open_optional_objects`] – keep `{}` distinct from absent.
//! 7. [`default_result_type`] – absent `resultType` means `"complete"`.
//! 8. [`default_cache_hints`] – absent `ttlMs`/`cacheScope` are defaulted.
//!
//! Each pass exists because typify silently lost a schema fact the rest of the
//! crate depends on; the pass docs say which one.
//!
//! ## `allOf` flattening
//!
//! typify (0.6 and 0.7) panics on `2025-11-25/schema.json` with
//! `assertion failed: merged_schema.metadata.is_none()` (`convert.rs:1435`):
//! its `allOf`-merge path rejects metadata that survives the merge, and the MCP
//! task result types (`CancelTaskResult`, `GetTaskResult`,
//! `TaskStatusNotificationParams`) are `allOf[Base, Task]` where the `Task`
//! `$ref` target carries a `description`. See `.strategy/v4/AUDIT_FINDINGS.md`
//! F14.
//!
//! Specialized pre-pass: flatten supported `allOf` intersection into a single inline object —
//! resolve each member (`$ref` → its definition, or inline subschema), drop
//! metadata at the merge site, and union `properties`/`required`/`type`/
//! unrestricted `additionalProperties`. Unsupported intersections fail generation
//! instead of dropping constraints. Typify never reaches the
//! offending merge path.

use serde_json::{Map, Value};

const METADATA_KEYS: [&str; 3] = ["description", "title", "default"];

/// Flatten every `allOf` in `schema`, in place. `$ref`s are resolved against a
/// snapshot of the schema's `$defs`/`definitions` taken before mutation.
pub fn flatten_all_of(schema: &mut Value) -> Result<(), String> {
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .cloned()
        .unwrap_or(Value::Null);
    // Transactional: an unsupported intersection leaves the input untouched.
    let mut candidate = schema.clone();
    walk(&mut candidate, &defs, 0)?;
    *schema = candidate;
    Ok(())
}

fn walk(value: &mut Value, defs: &Value, depth: usize) -> Result<(), String> {
    if depth > 128 {
        return Err("schema intersection recursion limit exceeded".into());
    }
    match value {
        Value::Object(map) => {
            if map.contains_key("allOf") {
                flatten_node(map, defs, depth + 1)?;
            }
            for child in map.values_mut() {
                walk(child, defs, depth + 1)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, defs, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn flatten_node(node: &mut Map<String, Value>, defs: &Value, depth: usize) -> Result<(), String> {
    if depth > 128 {
        return Err("cyclic or excessively deep allOf reference".into());
    }
    let Some(Value::Array(members)) = node.remove("allOf") else {
        return Err("allOf must be an array".into());
    };
    if members.is_empty() {
        return Err("allOf must not be empty".into());
    }
    let mut merged = Map::new();
    for member in members {
        let mut resolved = match member.get("$ref").and_then(Value::as_str) {
            Some(reference) => {
                if member.as_object().is_none_or(|m| m.len() != 1) {
                    return Err(
                        "allOf reference siblings require explicit intersection support".into(),
                    );
                }
                let name = reference
                    .strip_prefix("#/$defs/")
                    .or_else(|| reference.strip_prefix("#/definitions/"))
                    .ok_or("only local definition references can be flattened")?;
                let name = name.replace("~1", "/").replace("~0", "~");
                defs.get(&name)
                    .cloned()
                    .ok_or_else(|| format!("unresolved allOf reference: {reference}"))?
            }
            None => member,
        };
        let obj = resolved
            .as_object_mut()
            .ok_or("only object schemas can be flattened")?;
        if obj.contains_key("allOf") {
            flatten_node(obj, defs, depth + 1)?;
        }
        merge_object_into(&mut merged, obj)?;
    }
    // Parent constraints are part of the intersection, too.
    merge_object_into(&mut merged, node)?;
    for key in METADATA_KEYS {
        if let Some(value) = node.get(key) {
            merged.insert(key.into(), value.clone());
        }
    }
    *node = merged;
    Ok(())
}

fn merge_object_into(
    target: &mut Map<String, Value>,
    src: &Map<String, Value>,
) -> Result<(), String> {
    for (key, value) in src {
        match key.as_str() {
            "description" | "title" | "default" => {}
            "type" if value == "object" => {
                target.insert(key.clone(), value.clone());
            }
            "properties" => {
                let props = value.as_object().ok_or("properties must be an object")?;
                let dest = target
                    .entry(key.clone())
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .unwrap();
                for (name, schema) in props {
                    let combined = match dest.get(name) {
                        Some(prior) if prior != schema => intersect_property(prior, schema)
                            .ok_or_else(|| {
                                format!("unsupported intersection of property {name}")
                            })?,
                        _ => schema.clone(),
                    };
                    dest.insert(name.clone(), combined);
                }
            }
            "required" => {
                let items = value.as_array().ok_or("required must be an array")?;
                let dest = target
                    .entry(key.clone())
                    .or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()
                    .unwrap();
                for item in items {
                    if !item.is_string() {
                        return Err("required entries must be strings".into());
                    }
                    if !dest.contains(item) {
                        dest.push(item.clone());
                    }
                }
            }
            "additionalProperties"
                if value == &Value::Bool(true) || value.as_object().is_some_and(Map::is_empty) =>
            {
                target.entry(key.clone()).or_insert_with(|| value.clone());
            }
            _ => return Err(format!("unsupported allOf keyword or constraint: {key}")),
        }
    }
    Ok(())
}

// A schema containing a superset of another's identical constraints is their
// intersection. Annotations do not constrain instances. This covers the
// frozen error schema's integer code refined by `const`, without generalizing
// to conflicting bounds, enums, closed objects, or arbitrary intersections.
fn intersect_property(a: &Value, b: &Value) -> Option<Value> {
    fn constraints(value: &Value) -> Option<Map<String, Value>> {
        let mut map = value.as_object()?.clone();
        for key in METADATA_KEYS {
            map.remove(key);
        }
        Some(map)
    }
    let left = constraints(a)?;
    let right = constraints(b)?;
    if left
        .iter()
        .all(|(key, value)| right.get(key) == Some(value))
    {
        Some(b.clone())
    } else if right
        .iter()
        .all(|(key, value)| left.get(key) == Some(value))
    {
        Some(a.clone())
    } else {
        None
    }
}

/// Open every "schema-of-schema" node so typify keeps arbitrary JSON Schema
/// keywords instead of dropping them.
///
/// `Tool.inputSchema`/`outputSchema` describe a *nested* JSON Schema. The
/// `2025-11-25` schema models them with fixed `properties` (`$schema`,
/// `properties`, `required`, `type`) and **no** `additionalProperties`, so
/// typify emits a CLOSED struct — serializing a tool schema then silently drops
/// every other keyword (`$defs`, `additionalProperties`, `oneOf`, `if`/`then`,
/// …). That corrupts any tool whose argument schema uses them: a nested-type
/// argument advertises a `$ref` into a `$defs` that was dropped (a dangling
/// reference). The `draft` schema fixed this by adding `"additionalProperties":
/// {}`, which typify turns into a flattened `extra` catch-all. Backport that to
/// every version: inject `additionalProperties: {}` into any object node whose
/// `properties` pin a `type` field to `{const: "object"}` — the distinctive
/// shape of an embedded JSON Schema — and that doesn't already set
/// `additionalProperties`.
pub fn open_embedded_schemas(schema: &mut Value) {
    walk(schema);

    fn walk(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if is_embedded_schema(map) && !map.contains_key("additionalProperties") {
                    map.insert("additionalProperties".into(), Value::Object(Map::new()));
                }
                for child in map.values_mut() {
                    walk(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child);
                }
            }
            _ => {}
        }
    }

    /// A node describing an embedded JSON Schema: `type: "object"` whose
    /// `properties` pin a nested `type` field to `{const: "object"}`, as
    /// `inputSchema`/`outputSchema`/`requestedSchema` do.
    ///
    /// The `const: "object"` pin is the whole discriminator. An earlier version
    /// also required a `$schema` property, which holds for `2025-11-25` and the
    /// draft but **not** for `2025-06-18` — so that revision's tool schemas were
    /// left closed and would have silently dropped every keyword the struct
    /// doesn't name. Across all three schemas this predicate matches exactly the
    /// three schema-of-schema nodes and nothing else: a content block pins its
    /// `type` to `"text"`/`"image"`/…, never to `"object"`.
    fn is_embedded_schema(map: &Map<String, Value>) -> bool {
        if map.get("type").and_then(Value::as_str) != Some("object") {
            return false;
        }
        let Some(props) = map.get("properties").and_then(Value::as_object) else {
            return false;
        };
        props
            .get("type")
            .and_then(Value::as_object)
            .and_then(|t| t.get("const"))
            .and_then(Value::as_str)
            == Some("object")
    }
}

/// Open every `_meta` object definition so typify keeps arbitrary keys.
///
/// `_meta` is an open map by spec — `MetaObject` carries arbitrary
/// reverse-DNS-namespaced keys, and the specialized shapes (e.g.
/// `RequestMetaObject`) extend it with *reserved* keys while keeping the map
/// open. The set of specialized shapes moves with the spec, so they are
/// discovered from the schema rather than listed here. JSON Schema treats a `properties`-only object as
/// open, but typify emits a CLOSED struct for it, so round-tripping a message
/// through the typed structs would silently drop every non-reserved `_meta`
/// key (trace context, user metadata). Inject `additionalProperties: {}` into
/// every definition referenced by a `_meta` property so typify emits a
/// flattened `extra` catch-all map, mirroring `open_embedded_schemas`.
pub fn open_meta_objects(schema: &mut Value) {
    let mut targets: Vec<String> = Vec::new();
    collect_meta_refs(schema, &mut targets);

    let defs_key = if schema.get("$defs").is_some() {
        "$defs"
    } else {
        "definitions"
    };
    let Some(defs) = schema.get_mut(defs_key).and_then(Value::as_object_mut) else {
        return;
    };
    for name in targets {
        if let Some(Value::Object(def)) = defs.get_mut(&name)
            && def.get("type").and_then(Value::as_str) == Some("object")
            && def.contains_key("properties")
            && !def.contains_key("additionalProperties")
        {
            def.insert("additionalProperties".into(), Value::Object(Map::new()));
        }
    }

    /// Collect the local `$ref` targets of every property named `_meta`.
    fn collect_meta_refs(value: &Value, targets: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::Object(props)) = map.get("properties")
                    && let Some(meta) = props.get("_meta")
                    && let Some(reference) = meta.get("$ref").and_then(Value::as_str)
                    && let Some(name) = reference.rsplit('/').next()
                    && !targets.iter().any(|t| t == name)
                {
                    targets.push(name.to_owned());
                }
                for child in map.values() {
                    collect_meta_refs(child, targets);
                }
            }
            Value::Array(items) => {
                for child in items {
                    collect_meta_refs(child, targets);
                }
            }
            _ => {}
        }
    }
}

/// Name of the open-object definition every presence marker is pointed at.
/// `2026-07-28` ships one under this name; the older revisions get one
/// synthesized with the same meaning.
const OPEN_OBJECT: &str = "JSONObject";

fn defs_key(schema: &Value) -> &'static str {
    if schema.get("$defs").is_some() {
        "$defs"
    } else {
        "definitions"
    }
}

/// Turn every string `const` into a one-value `enum`.
///
/// typify ignores `const` and emits a plain `String`, so the discriminator of
/// every tagged shape (`type: "audio"`, `method: "ping"`, `mode: "url"`) stops
/// constraining anything. The unions over those shapes are `untagged`, so serde
/// picks the first variant whose fields fit: an audio block has the same fields
/// as an image block and decodes as one, and every request after `initialize`
/// decodes as `PingRequest`. A one-value `enum` becomes a one-variant Rust enum,
/// which makes each variant match exactly its own tag.
///
/// The `type: {const: "object"}` pin of an embedded JSON Schema is left alone:
/// it is not a discriminator, [`open_embedded_schemas`] keys off it, and a tool
/// schema is user data that decoding should not start rejecting.
pub fn pin_string_consts(schema: &mut Value) {
    walk(schema);

    fn walk(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if map.get("type").and_then(Value::as_str) == Some("string")
                    && let Some(Value::String(pinned)) = map.get("const")
                    && pinned != "object"
                {
                    let pinned = Value::String(pinned.clone());
                    map.remove("const");
                    map.insert("enum".into(), Value::Array(vec![pinned]));
                }
                for child in map.values_mut() {
                    walk(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child);
                }
            }
            _ => {}
        }
    }
}

/// Make `{}` distinguishable from absent for every optional open object.
///
/// A capability sub-object such as `elicitation.url` or `sampling.tools` is a
/// presence marker: an open object whose only information is that it exists.
/// typify renders an inline open object as a `Map` that is skipped when empty,
/// so `{"elicitation":{"url":{}}}` re-serializes as `{"elicitation":{}}` and an
/// empty `structuredContent` disappears. Pointed at a named definition instead,
/// the same property becomes `Option<JsonObject>`, which keeps the difference.
///
/// This also replaces `2026-07-28`'s `JSONValue`, which admits only strings,
/// integers and booleans, with an unconstrained schema. Extension settings,
/// `experimental` capabilities and sampling `metadata` are arbitrary JSON; a
/// float or a `null` in any of them made the typed decode fail.
pub fn open_optional_objects(schema: &mut Value) {
    let key = defs_key(schema);
    let pointer = format!("#/{key}/{OPEN_OBJECT}");
    let mut rewritten = 0usize;
    walk(schema, &pointer, &mut rewritten);

    let Some(defs) = schema.get_mut(key).and_then(Value::as_object_mut) else {
        return;
    };
    if defs.contains_key("JSONValue") {
        defs.insert(
            "JSONValue".into(),
            serde_json::json!({ "description": "Any JSON value." }),
        );
    }
    if rewritten > 0 || defs.contains_key(OPEN_OBJECT) {
        defs.insert(
            OPEN_OBJECT.into(),
            serde_json::json!({
                "description": "An arbitrary JSON object.",
                "type": "object",
                "additionalProperties": {}
            }),
        );
    }

    fn walk(value: &mut Value, pointer: &str, rewritten: &mut usize) {
        match value {
            Value::Object(map) => {
                let required: Vec<String> = map
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|r| {
                        r.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(Value::Object(props)) = map.get_mut("properties") {
                    for (name, prop) in props.iter_mut() {
                        if name != "_meta" && !required.contains(name) && is_presence_marker(prop) {
                            let mut replacement = Map::new();
                            replacement.insert("$ref".into(), Value::String(pointer.to_owned()));
                            if let Some(description) = prop.get("description") {
                                replacement.insert("description".into(), description.clone());
                            }
                            *prop = Value::Object(replacement);
                            *rewritten += 1;
                        }
                    }
                }
                for child in map.values_mut() {
                    walk(child, pointer, rewritten);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, pointer, rewritten);
                }
            }
            _ => {}
        }
    }

    /// `{"type":"object"}` with no declared properties and nothing constraining
    /// the rest: the shape of a presence marker or of an arbitrary JSON object.
    fn is_presence_marker(schema: &Value) -> bool {
        let Some(map) = schema.as_object() else {
            return false;
        };
        let open_rest = match map.get("additionalProperties") {
            None | Some(Value::Bool(true)) => true,
            Some(Value::Object(inner)) => inner.is_empty(),
            Some(_) => false,
        };
        let no_properties = match map.get("properties") {
            None => true,
            Some(Value::Object(props)) => props.is_empty(),
            Some(_) => false,
        };
        map.get("type").and_then(Value::as_str) == Some("object")
            && open_rest
            && no_properties
            && map.keys().all(|k| {
                matches!(
                    k.as_str(),
                    "type" | "properties" | "additionalProperties" | "description" | "title"
                )
            })
    }
}

/// Let the `properties` of an embedded JSON Schema hold any subschema.
///
/// JSON Schema 2020-12 allows a boolean subschema, and schemars emits `true` for
/// a `serde_json::Value` field (rmcp does the same). The `2025-06-18` and
/// `2025-11-25` schemas type each property schema as an object, so one `true`
/// failed the typed parse of the whole tool: the server fell back to a bare
/// `{"type":"object"}` and a client's `tools/list` failed outright.
pub fn allow_boolean_subschemas(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            let embedded = map.get("type").and_then(Value::as_str) == Some("object")
                && map
                    .get("properties")
                    .and_then(Value::as_object)
                    .and_then(|p| p.get("type"))
                    .and_then(|t| t.get("const"))
                    .and_then(Value::as_str)
                    == Some("object");
            if embedded
                && let Some(Value::Object(nested)) = map
                    .get_mut("properties")
                    .and_then(|p| p.get_mut("properties"))
                && nested.contains_key("additionalProperties")
            {
                nested.insert("additionalProperties".into(), Value::Object(Map::new()));
            }
            for child in map.values_mut() {
                allow_boolean_subschemas(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                allow_boolean_subschemas(child);
            }
        }
        _ => {}
    }
}

/// Make `resultType` discriminate, and default it where it may be absent.
///
/// `2026-07-28` makes `resultType` required on the wire but says "clients
/// **MUST** treat an absent `resultType` as `"complete"`" for servers on earlier
/// revisions. Required in the schema, typify made its absence a decode error.
///
/// The schema types `resultType` as an open string everywhere, including on
/// `InputRequiredResult`, which the MRTR prose defines as the result whose
/// `resultType` is `"input_required"`. Unpinned, the untagged
/// `anyOf[InputRequiredResult, CallToolResult]` in every `*ResultResponse`
/// matched `InputRequiredResult` first (all its other fields are optional) and
/// dropped the real result. Pinned, it is required and exact, so a complete
/// result falls through to the type that can hold it.
pub fn default_result_type(schema: &mut Value) {
    let key = defs_key(schema);
    if let Some(prop) = schema
        .get_mut(key)
        .and_then(|defs| defs.get_mut("InputRequiredResult"))
        .and_then(|def| def.get_mut("properties"))
        .and_then(|props| props.get_mut("resultType"))
        .and_then(Value::as_object_mut)
    {
        prop.insert("enum".into(), serde_json::json!(["input_required"]));
    }
    walk(schema);

    fn walk(schema: &mut Value) {
        match schema {
            Value::Object(map) => {
                let defaultable = map
                    .get("properties")
                    .and_then(|p| p.get("resultType"))
                    .is_some_and(|r| r.get("enum").is_none());
                if defaultable {
                    if let Some(Value::Array(required)) = map.get_mut("required") {
                        required.retain(|r| r != "resultType");
                    }
                    if let Some(Value::Object(prop)) = map
                        .get_mut("properties")
                        .and_then(|p| p.get_mut("resultType"))
                    {
                        prop.insert("default".into(), Value::String("complete".into()));
                    }
                }
                for child in map.values_mut() {
                    walk(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child);
                }
            }
            _ => {}
        }
    }
}

/// Default absent caching hints instead of refusing the result.
///
/// `2026-07-28` requires `ttlMs` and `cacheScope` of servers but tells clients
/// "If `ttlMs` is absent, clients **SHOULD** assume a default of `0`
/// (immediately stale) … This should only occur in older server versions."
/// Required in the schema, typify made every such result a decode error (one
/// of the schema's own examples omits both). `cacheScope` gets `"private"`: the
/// reading that never shares a response across authorization contexts.
pub fn default_cache_hints(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            let hinted = map
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|p| p.contains_key("ttlMs") && p.contains_key("cacheScope"));
            if hinted {
                if let Some(Value::Array(required)) = map.get_mut("required") {
                    required.retain(|r| r != "ttlMs" && r != "cacheScope");
                }
                if let Some(Value::Object(props)) = map.get_mut("properties") {
                    for (name, default) in
                        [("ttlMs", json_zero()), ("cacheScope", "private".into())]
                    {
                        if let Some(Value::Object(prop)) = props.get_mut(name) {
                            prop.insert("default".into(), default);
                        }
                    }
                }
            }
            for child in map.values_mut() {
                default_cache_hints(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                default_cache_hints(child);
            }
        }
        _ => {}
    }

    fn json_zero() -> Value {
        Value::Number(0.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cache_hints_default_rather_than_being_required() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "ttlMs": { "type": "integer" },
                "cacheScope": { "$ref": "#/$defs/CacheScope" },
                "tools": { "type": "array" }
            },
            "required": ["cacheScope", "tools", "ttlMs"]
        });
        default_cache_hints(&mut schema);
        assert_eq!(schema["required"], json!(["tools"]));
        assert_eq!(schema["properties"]["ttlMs"]["default"], json!(0));
        assert_eq!(
            schema["properties"]["cacheScope"]["default"],
            json!("private")
        );
    }

    #[test]
    fn string_consts_become_one_value_enums_except_the_embedded_schema_pin() {
        let mut schema = json!({
            "$defs": {
                "AudioContent": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "audio", "type": "string" },
                        // A property *named* `const` is not the keyword.
                        "const": { "description": "x", "type": "string" }
                    }
                },
                "Tool": {
                    "type": "object",
                    "properties": {
                        "inputSchema": {
                            "type": "object",
                            "properties": { "type": { "const": "object", "type": "string" } }
                        }
                    }
                },
                "Code": { "const": -32042, "type": "integer" }
            }
        });
        pin_string_consts(&mut schema);
        let audio = &schema["$defs"]["AudioContent"]["properties"];
        assert_eq!(
            audio["type"],
            json!({ "enum": ["audio"], "type": "string" })
        );
        assert_eq!(
            audio["const"],
            json!({ "description": "x", "type": "string" })
        );
        assert_eq!(
            schema["$defs"]["Tool"]["properties"]["inputSchema"]["properties"]["type"],
            json!({ "const": "object", "type": "string" })
        );
        assert_eq!(
            schema["$defs"]["Code"],
            json!({ "const": -32042, "type": "integer" })
        );
    }

    #[test]
    fn optional_presence_markers_point_at_a_named_open_object() {
        let mut schema = json!({
            "$defs": {
                "Caps": {
                    "type": "object",
                    "properties": {
                        "url": { "additionalProperties": true, "properties": {}, "type": "object" },
                        "structured": { "additionalProperties": {}, "type": "object", "description": "d" },
                        "input": { "additionalProperties": {}, "type": "object" },
                        "_meta": { "additionalProperties": {}, "type": "object" },
                        "typed": { "additionalProperties": { "type": "string" }, "type": "object" },
                        "shaped": { "properties": { "x": {} }, "type": "object" }
                    },
                    "required": ["input"]
                }
            }
        });
        open_optional_objects(&mut schema);
        let props = &schema["$defs"]["Caps"]["properties"];
        assert_eq!(props["url"], json!({ "$ref": "#/$defs/JSONObject" }));
        assert_eq!(
            props["structured"],
            json!({ "$ref": "#/$defs/JSONObject", "description": "d" })
        );
        assert_eq!(
            props["input"]["type"],
            json!("object"),
            "required stays inline"
        );
        assert_eq!(
            props["_meta"]["type"],
            json!("object"),
            "_meta stays inline"
        );
        assert_eq!(
            props["typed"]["type"],
            json!("object"),
            "a typed map stays inline"
        );
        assert_eq!(
            props["shaped"]["type"],
            json!("object"),
            "a shaped object stays inline"
        );
        assert_eq!(
            schema["$defs"]["JSONObject"],
            json!({ "description": "An arbitrary JSON object.", "type": "object", "additionalProperties": {} })
        );
    }

    #[test]
    fn json_value_becomes_unconstrained() {
        let mut schema = json!({
            "$defs": {
                "JSONObject": { "additionalProperties": { "$ref": "#/$defs/JSONValue" }, "type": "object" },
                "JSONValue": { "anyOf": [ { "type": ["string", "integer", "boolean"] } ] }
            }
        });
        open_optional_objects(&mut schema);
        assert_eq!(
            schema["$defs"]["JSONValue"],
            json!({ "description": "Any JSON value." })
        );
        assert_eq!(
            schema["$defs"]["JSONObject"]["additionalProperties"],
            json!({})
        );
    }

    #[test]
    fn embedded_schema_properties_accept_boolean_subschemas() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "properties": {
                    "additionalProperties": { "additionalProperties": true, "properties": {}, "type": "object" },
                    "type": "object"
                },
                "type": { "const": "object", "type": "string" }
            }
        });
        allow_boolean_subschemas(&mut schema);
        assert_eq!(
            schema["properties"]["properties"]["additionalProperties"],
            json!({})
        );
    }

    #[test]
    fn result_type_defaults_to_complete_except_on_input_required_results() {
        let result = json!({
            "type": "object",
            "properties": { "resultType": { "type": "string" } },
            "required": ["resultType", "tools"]
        });
        let mut schema = json!({
            "$defs": { "ListToolsResult": result.clone(), "InputRequiredResult": result }
        });
        default_result_type(&mut schema);
        let list = &schema["$defs"]["ListToolsResult"];
        assert_eq!(list["required"], json!(["tools"]));
        assert_eq!(
            list["properties"]["resultType"]["default"],
            json!("complete")
        );
        let input = &schema["$defs"]["InputRequiredResult"];
        assert_eq!(input["required"], json!(["resultType", "tools"]));
        assert_eq!(
            input["properties"]["resultType"],
            json!({ "type": "string", "enum": ["input_required"] })
        );
    }

    #[test]
    fn flattens_allof_of_refs_and_drops_member_metadata() {
        let mut schema = json!({
            "$defs": {
                "Base": { "type": "object", "properties": { "a": { "type": "string" } } },
                "Task": {
                    "type": "object",
                    "description": "a task",
                    "properties": { "b": { "type": "number" } },
                    "required": ["b"]
                },
                "Result": {
                    "description": "the result",
                    "allOf": [ { "$ref": "#/$defs/Base" }, { "$ref": "#/$defs/Task" } ]
                }
            }
        });
        flatten_all_of(&mut schema).unwrap();
        let result = &schema["$defs"]["Result"];
        assert!(result.get("allOf").is_none(), "allOf removed");
        // Parent metadata preserved.
        assert_eq!(result["description"], json!("the result"));
        // Member properties merged.
        assert!(result["properties"].get("a").is_some());
        assert!(result["properties"].get("b").is_some());
        assert_eq!(result["required"], json!(["b"]));
        assert_eq!(result["type"], json!("object"));
        // The standalone Task definition keeps its own description.
        assert_eq!(schema["$defs"]["Task"]["description"], json!("a task"));
    }

    #[test]
    fn opens_embedded_schema_nodes() {
        // A `2025-11-25`-style closed inputSchema node gains `additionalProperties`.
        let mut schema = json!({
            "$defs": {
                "Tool": {
                    "type": "object",
                    "properties": {
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "$schema": { "type": "string" },
                                "properties": { "type": "object" },
                                "type": { "const": "object", "type": "string" }
                            }
                        },
                        // A plain object property (no `$schema`, no type-const) is
                        // left closed.
                        "name": { "type": "object", "properties": { "x": { "type": "string" } } }
                    }
                }
            }
        });
        open_embedded_schemas(&mut schema);
        let input = &schema["$defs"]["Tool"]["properties"]["inputSchema"];
        assert_eq!(input["additionalProperties"], json!({}), "opened");
        let name = &schema["$defs"]["Tool"]["properties"]["name"];
        assert!(
            name.get("additionalProperties").is_none(),
            "a non-schema object stays closed"
        );
    }

    #[test]
    fn opens_meta_object_definitions() {
        let mut schema = json!({
            "$defs": {
                "RequestMetaObject": {
                    "type": "object",
                    "properties": {
                        "io.modelcontextprotocol/protocolVersion": { "type": "string" }
                    }
                },
                // Same shape but never referenced from a `_meta` property —
                // must stay closed.
                "NotMeta": {
                    "type": "object",
                    "properties": { "x": { "type": "string" } }
                },
                "SomeResult": {
                    "type": "object",
                    "properties": {
                        "_meta": { "$ref": "#/$defs/RequestMetaObject" },
                        "other": { "$ref": "#/$defs/NotMeta" }
                    }
                }
            }
        });
        open_meta_objects(&mut schema);
        assert_eq!(
            schema["$defs"]["RequestMetaObject"]["additionalProperties"],
            json!({}),
            "meta object opened"
        );
        assert!(
            schema["$defs"]["NotMeta"]
                .get("additionalProperties")
                .is_none(),
            "non-meta object stays closed"
        );
    }

    #[test]
    fn open_embedded_schemas_is_idempotent() {
        // A node that already sets `additionalProperties` (the draft shape) is
        // left untouched.
        let mut schema = json!({
            "type": "object",
            "additionalProperties": true,
            "properties": {
                "$schema": { "type": "string" },
                "type": { "const": "object", "type": "string" }
            }
        });
        open_embedded_schemas(&mut schema);
        assert_eq!(schema["additionalProperties"], json!(true), "unchanged");
    }

    /// `2025-06-18` describes `inputSchema` **without** a `$schema` property.
    /// Requiring one left that revision's tool schemas closed, so typify emitted
    /// a struct with no catch-all and every keyword it doesn't name — `$defs`,
    /// `oneOf`, `additionalProperties` — was dropped on serialization. Any tool
    /// taking a nested type would have advertised a `$ref` into a `$defs` that
    /// no longer existed.
    #[test]
    fn an_embedded_schema_without_a_dollar_schema_property_is_still_opened() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "properties": { "type": "object" },
                "required": { "items": { "type": "string" }, "type": "array" },
                "type": { "const": "object", "type": "string" }
            }
        });
        open_embedded_schemas(&mut schema);
        assert_eq!(schema["additionalProperties"], json!({}));
    }

    /// The `const: "object"` pin is what distinguishes a schema-of-schema node.
    /// A content block pins its `type` to its own tag, and must stay closed —
    /// otherwise every wire struct grows a catch-all and typed round-trips stop
    /// being able to reject unknown fields.
    #[test]
    fn a_tagged_object_that_is_not_a_schema_stays_closed() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "type": { "const": "text", "type": "string" }
            }
        });
        open_embedded_schemas(&mut schema);
        assert!(schema.get("additionalProperties").is_none());
    }
}

#[cfg(test)]
mod audit_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_unsupported_intersections_transactionally() {
        for schema in [
            json!({"allOf":[{"type":"string"},{"type":"number"}]}),
            json!({"allOf":[{"properties":{"x":{"minimum":5}}},{"properties":{"x":{"maximum":3}}}]}),
            json!({"allOf":[{"additionalProperties":false},{"properties":{"x":{}}}]}),
            json!({"allOf":[{"$ref":"https://example.com/schema"}]}),
            json!({"$defs":{"x":{"allOf":[{"$ref":"#/$defs/x"}]}},"allOf":[{"$ref":"#/$defs/x"}]}),
        ] {
            let mut candidate = schema.clone();
            assert!(flatten_all_of(&mut candidate).is_err());
            assert_eq!(candidate, schema);
        }
    }
}
