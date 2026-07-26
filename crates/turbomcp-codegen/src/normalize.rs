//! Schema normalization pre-pass.
//!
//! typify 0.6 panics on `2025-11-25/schema.json` with
//! `assertion failed: merged_schema.metadata.is_none()` (`convert.rs:1435`):
//! its `allOf`-merge path rejects metadata that survives the merge, and the MCP
//! task result types (`CancelTaskResult`, `GetTaskResult`,
//! `TaskStatusNotificationParams`) are `allOf[Base, Task]` where the `Task`
//! `$ref` target carries a `description`. See `.strategy/v4/AUDIT_FINDINGS.md`
//! F14.
//!
//! Fix: flatten every `allOf` intersection into a single inline object —
//! resolve each member (`$ref` → its definition, or inline subschema), drop
//! metadata at the merge site, and union `properties`/`required`/`type`/
//! `additionalProperties`. Semantically identical (an intersection of object
//! schemas is one object with all their fields), and typify never reaches the
//! offending merge path. Validated: 2025-11-25 → 25,473 lines, draft → 22,506,
//! both compile.

use serde_json::{Map, Value};

const METADATA_KEYS: [&str; 3] = ["description", "title", "default"];

/// Flatten every `allOf` in `schema`, in place. `$ref`s are resolved against a
/// snapshot of the schema's `$defs`/`definitions` taken before mutation.
pub fn flatten_all_of(schema: &mut Value) {
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .cloned()
        .unwrap_or(Value::Null);
    walk(schema, &defs);
}

fn walk(value: &mut Value, defs: &Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("allOf") {
                flatten_node(map, defs);
            }
            for child in map.values_mut() {
                walk(child, defs);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, defs);
            }
        }
        _ => {}
    }
}

/// Resolve a local `$ref` (e.g. `#/$defs/Task`) to a clone of its target.
fn resolve(defs: &Value, reference: &str) -> Option<Value> {
    let name = reference.rsplit('/').next()?;
    defs.get(name).cloned()
}

fn flatten_node(node: &mut Map<String, Value>, defs: &Value) {
    let members = match node.remove("allOf") {
        Some(Value::Array(members)) => members,
        // Not the shape we flatten — restore and leave it for typify.
        Some(other) => {
            node.insert("allOf".into(), other);
            return;
        }
        None => return,
    };

    let mut merged = Map::new();
    for member in members {
        let mut resolved = match member.get("$ref").and_then(Value::as_str) {
            Some(reference) => resolve(defs, reference).unwrap_or_else(|| member.clone()),
            None => member.clone(),
        };
        if let Value::Object(obj) = &mut resolved {
            // Defensive: a resolved target could itself be an intersection.
            if obj.contains_key("allOf") {
                flatten_node(obj, defs);
            }
            for key in METADATA_KEYS {
                obj.remove(key);
            }
            merge_object_into(&mut merged, obj);
        }
    }

    // Apply merged content without clobbering the node's own keys, so the
    // node's own `description`/`type` (the parent metadata) is preserved.
    for (key, value) in merged {
        node.entry(key).or_insert(value);
    }
}

fn merge_object_into(target: &mut Map<String, Value>, src: &Map<String, Value>) {
    if let Some(Value::Object(props)) = src.get("properties") {
        let entry = target
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(target_props) = entry {
            for (key, value) in props {
                target_props.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(Value::Array(required)) = src.get("required") {
        let entry = target
            .entry("required")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(target_required) = entry {
            for value in required {
                if !target_required.contains(value) {
                    target_required.push(value.clone());
                }
            }
        }
    }
    if let Some(ty) = src.get("type") {
        target.insert("type".into(), ty.clone());
    }
    if let Some(additional) = src.get("additionalProperties") {
        target
            .entry("additionalProperties")
            .or_insert_with(|| additional.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        flatten_all_of(&mut schema);
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
