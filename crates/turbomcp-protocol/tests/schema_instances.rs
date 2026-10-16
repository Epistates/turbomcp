//! Every schema definition, in every revision, through its generated type.
//!
//! For each definition the schema itself yields instances: the smallest one
//! (only `required` fields) and the largest (every field, one item per array,
//! one entry per open map), for each member of a top-level union. Each must
//! conform (which checks the generator), decode into the definition's
//! generated type, re-encode to JSON that still conforms, and come back with
//! nothing changed. The generated types are the only path from bytes to the
//! neutral layer, so a field a conversion or a normalize pass loses shows up
//! here as a named change.
//!
//! The definition → type table is `tests/registry/<revision>.rs`, written by
//! `just codegen` and drift-checked with the types, so a definition upstream
//! adds is covered the day it is generated.

mod common;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use common::{Oracle, changes};

type Case = (&'static str, fn(&Value) -> Result<Value, String>);

fn decode_encode<T: Serialize + DeserializeOwned>(instance: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(instance.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

macro_rules! registry {
    ($($def:literal => $ty:ident,)*) => {
        pub fn cases() -> Vec<super::Case> {
            vec![$(($def, super::decode_encode::<$ty>),)*]
        }
    };
}

mod v0618 {
    use turbomcp_protocol::v2025_06_18::types::*;
    include!("registry/2025-06-18.rs");
}
mod v1125 {
    use turbomcp_protocol::v2025_11_25::types::*;
    include!("registry/2025-11-25.rs");
}
mod v0728 {
    use turbomcp_protocol::v2026_07_28::types::*;
    include!("registry/2026-07-28.rs");
}

/// The definitions codegen opens (`normalize::open_results`).
const OPEN: [fn(&str) -> bool; 2] = [
    |name| name.ends_with("Result"),
    |name| {
        [
            "Tool",
            "Resource",
            "ResourceTemplate",
            "Prompt",
            "Implementation",
        ]
        .contains(&name)
    },
];

#[derive(Clone, Copy, PartialEq)]
enum Size {
    Smallest,
    Largest,
}

/// Instances of a schema, built from the schema.
struct Instances<'a> {
    oracle: &'a Oracle,
}

impl Instances<'_> {
    /// The instances of definition `name`: both sizes, per top-level union
    /// member.
    fn of(&self, name: &str) -> Vec<Value> {
        let def = self.oracle.def(name);
        let members: Vec<&Value> = match def.get("anyOf").and_then(Value::as_array) {
            Some(members) => members.iter().collect(),
            None => vec![def],
        };
        let mut out = Vec::new();
        for member in members {
            for size in [Size::Smallest, Size::Largest] {
                let mut instance = prose(name, self.build(member, size, 0));
                // A result or catalogue item is open (`schema.ts`'s
                // `[key: string]: unknown`); the largest instance carries a key
                // no revision defines, which must come back.
                if size == Size::Largest
                    && OPEN.iter().any(|open| open(name))
                    && let Value::Object(fields) = &mut instance
                {
                    fields.insert("x-vendor".into(), json!({ "trace": [1, "two"] }));
                }
                if !out.contains(&instance) {
                    out.push(instance);
                }
            }
        }
        out
    }

    /// `depth` counts `$ref` hops, where recursion lives.
    fn build(&self, schema: &Value, size: Size, depth: usize) -> Value {
        // Recursive definitions (a JSON Schema inside a tool) bottom out small.
        let size = if depth > 8 { Size::Smallest } else { size };
        let Some(map) = schema.as_object() else {
            return json!("any"); // `true`
        };
        if let Some(target) = map.get("$ref").and_then(Value::as_str) {
            let name = target.rsplit('/').next().expect("a local $ref");
            return prose(name, self.build(self.oracle.def(name), size, depth + 1));
        }
        if let Some(value) = map.get("const") {
            return value.clone();
        }
        if let Some(first) = map.get("enum").and_then(|e| e.get(0)) {
            return first.clone();
        }
        if let Some(first) = map.get("anyOf").and_then(|m| m.get(0)) {
            return self.build(first, size, depth);
        }
        if let Some(members) = map.get("allOf").and_then(Value::as_array) {
            let mut merged = Map::new();
            for member in members {
                if let Value::Object(part) = self.build(member, size, depth) {
                    merged.extend(part);
                }
            }
            return Value::Object(merged);
        }
        let ty = match map.get("type") {
            Some(Value::String(ty)) => ty.as_str(),
            Some(Value::Array(types)) => types[0].as_str().expect("a type name"),
            _ if map.contains_key("properties") => "object",
            _ => return json!("any"),
        };
        match ty {
            "object" => self.object(map, size, depth),
            "array" => {
                let wanted = size == Size::Largest
                    || map.get("minItems").and_then(Value::as_u64).unwrap_or(0) > 0;
                match map.get("items") {
                    Some(items) if wanted => json!([self.build(items, size, depth)]),
                    _ => json!([]),
                }
            }
            "string" => match map.get("format").and_then(Value::as_str) {
                Some("uri") => json!("https://example.com/a"),
                Some("uri-template") => json!("file:///{path}"),
                Some("byte") => json!("AAAA"),
                _ => json!("text"),
            },
            "integer" => json!(map.get("minimum").and_then(Value::as_i64).unwrap_or(1)),
            "number" => json!(0.5),
            "boolean" => json!(true),
            "null" => Value::Null,
            other => panic!("{}: no instance for type {other}", self.oracle.rev),
        }
    }

    fn object(&self, map: &Map<String, Value>, size: Size, depth: usize) -> Value {
        let required: Vec<&str> = map
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut out = Map::new();
        if let Some(props) = map.get("properties").and_then(Value::as_object) {
            for (name, prop) in props {
                if size == Size::Largest || required.contains(&name.as_str()) {
                    out.insert(name.clone(), self.build(prop, size, depth));
                }
            }
        }
        if size == Size::Largest
            && let Some(rest @ Value::Object(_)) = map.get("additionalProperties")
        {
            out.insert("extra".into(), self.build(rest, size, depth));
        }
        Value::Object(out)
    }
}

/// What the prose says that the schema does not. `resultType` is an open
/// string in the schema, but the MRTR text makes an input-required result
/// exactly the one whose `resultType` is `"input_required"` (and the generated
/// type holds it to that), and every other result `"complete"`.
fn prose(name: &str, mut instance: Value) -> Value {
    if let Some(result_type) = instance.get_mut("resultType") {
        *result_type = json!(if name == "InputRequiredResult" {
            "input_required"
        } else {
            "complete"
        });
    }
    instance
}

/// Run every definition of one revision; `expected` lists, per definition,
/// the changes a round trip is allowed to make, each with its reason.
fn every_definition(oracle: &Oracle, cases: Vec<Case>, expected: &[(&str, &str)]) {
    let instances = Instances { oracle };
    let mut failures = Vec::new();
    let mut covered = 0;
    for (def, round_trip) in &cases {
        // Codegen synthesizes `JSONObject` for the revisions that lack one.
        if !oracle.defs().contains_key(*def) {
            assert_eq!(
                *def, "JSONObject",
                "{}: {def} is not in the schema",
                oracle.rev
            );
            continue;
        }
        for instance in instances.of(def) {
            let invalid = oracle.violations(def, &instance);
            assert!(
                invalid.is_empty(),
                "{} {def}: the generator built a nonconforming instance {instance}:\n  {}",
                oracle.rev,
                invalid.join("\n  ")
            );
            let back = match round_trip(&instance) {
                Ok(back) => back,
                Err(e) => {
                    failures.push(format!("{def}: does not decode {instance}: {e}"));
                    continue;
                }
            };
            for violation in oracle.violations(def, &back) {
                failures.push(format!("{def}: re-encodes nonconforming JSON: {violation}"));
            }
            for change in changes(&instance, &back) {
                if !expected.iter().any(|(d, c)| *d == *def && change == *c) {
                    failures.push(format!("{def}: {change}"));
                }
            }
            covered += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{}: {} of {covered} instances failed:\n{}",
        oracle.rev,
        failures.len(),
        failures.join("\n")
    );
    // Every definition the schema has is in the registry, except the unions
    // codegen drops (no type can hold them honestly).
    let dropped = [
        "ClientResult",
        "ServerResult",
        "PrimitiveSchemaDefinition",
        "EnumSchema",
        "MultiSelectEnumSchema",
    ];
    for def in oracle.defs().keys() {
        assert!(
            cases.iter().any(|(d, _)| d == def) || dropped.contains(&def.as_str()),
            "{}: {def} has no generated type in the registry",
            oracle.rev
        );
    }
}

#[test]
fn every_2025_06_18_definition_round_trips() {
    every_definition(&Oracle::load("2025-06-18"), v0618::cases(), &[]);
}

#[test]
fn every_2025_11_25_definition_round_trips() {
    every_definition(&Oracle::load("2025-11-25"), v1125::cases(), &[]);
}

#[test]
fn every_2026_07_28_definition_round_trips() {
    every_definition(&Oracle::load("2026-07-28"), v0728::cases(), &[]);
}
