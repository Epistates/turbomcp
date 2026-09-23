//! OpenAPI 3.0 Schema Objects as JSON Schema 2020-12.
//!
//! MCP reads a tool's `inputSchema` and `outputSchema` as JSON Schema 2020-12
//! (the default dialect when no `$schema` is given, and the one this crate
//! declares). An OpenAPI 3.0 Schema Object is close to that, but not the same,
//! and the differences are ones a validator notices:
//!
//! - `nullable: true` is how 3.0 says "or null"; 2020-12 has no such keyword
//!   and spells it with a `"null"` type.
//! - `exclusiveMinimum` / `exclusiveMaximum` are booleans modifying
//!   `minimum` / `maximum` in 3.0, and numbers in their own right in 2020-12.
//! - `example` is `examples` (an array) in 2020-12.
//! - `discriminator`, `xml`, `externalDocs` and `x-` extensions are OpenAPI
//!   vocabulary, which strict validators reject as unknown keywords.
//!
//! References into `components.schemas` are inlined, so a client that does not
//! resolve `$ref` still sees the whole shape. A recursive schema cannot be
//! inlined, so where expansion meets a schema it is already inside, it emits a
//! `$ref` into the root's `$defs` and the definition is written there. Nothing
//! else is left as a `$ref`; a reference that points nowhere becomes the empty
//! schema rather than a pointer no validator can follow.

use std::collections::BTreeSet;

use openapiv3::{OpenAPI, ReferenceOr, Schema};
use serde_json::{Map, Value, json};

/// Where OpenAPI keeps reusable schemas.
const COMPONENT_PREFIX: &str = "#/components/schemas/";

/// Longest chain of components that merely alias another one.
const MAX_ALIAS_DEPTH: usize = 10;

/// Converts an operation's schemas against one spec's components.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SchemaConverter<'a> {
    spec: &'a OpenAPI,
}

impl<'a> SchemaConverter<'a> {
    /// A converter resolving references against `spec`'s components.
    pub(crate) fn new(spec: &'a OpenAPI) -> Self {
        Self { spec }
    }

    /// Convert a schema into a self-contained JSON Schema 2020-12 value.
    ///
    /// Any `$defs` the result needs sit at its root. When the value is then
    /// nested in a larger schema, as a tool's input properties are, those
    /// `$defs` must be moved to the enclosing root, which is what `#/$defs/…`
    /// resolves against; see [`hoist_defs`].
    pub(crate) fn convert(&self, schema: &ReferenceOr<Schema>) -> Option<Value> {
        let value = match schema {
            ReferenceOr::Item(s) => serde_json::to_value(s).ok()?,
            ReferenceOr::Reference { reference } => json!({ "$ref": reference }),
        };

        let mut needed = BTreeSet::new();
        let mut root = self.convert_value(value, &mut Vec::new(), &mut needed);

        // Each definition is converted with only itself on the stack, so its
        // own recursion comes out as a `$ref` too, and may name more
        // definitions. The set of components is finite, so this ends.
        let mut defs = Map::new();
        while let Some(name) = needed.iter().find(|n| !defs.contains_key(**n)).copied() {
            let definition = self
                .component_named(name)
                .and_then(|(_, schema)| serde_json::to_value(schema).ok())
                .map_or_else(
                    || json!({}),
                    |value| self.convert_value(value, &mut vec![name], &mut needed),
                );
            defs.insert(name.to_string(), definition);
        }
        if !defs.is_empty()
            && let Value::Object(map) = &mut root
        {
            map.insert("$defs".to_string(), Value::Object(defs));
        }

        Some(root)
    }

    /// Convert one schema, inlining references that do not recurse.
    ///
    /// `stack` holds the components being expanded around this point; meeting
    /// one of them again is recursion, which is recorded in `needed` and left
    /// as a `$ref`.
    fn convert_value(
        &self,
        schema: Value,
        stack: &mut Vec<&'a str>,
        needed: &mut BTreeSet<&'a str>,
    ) -> Value {
        let Value::Object(mut map) = schema else {
            return schema;
        };

        if let Some(Value::String(reference)) = map.get("$ref") {
            // In OpenAPI 3.0 keywords beside a `$ref` are ignored, so the
            // reference is the whole schema.
            let Some((name, target)) = self.component(reference) else {
                return json!({});
            };
            if stack.contains(&name) {
                needed.insert(name);
                return json!({ "$ref": format!("#/$defs/{name}") });
            }
            let Ok(target) = serde_json::to_value(target) else {
                return json!({});
            };
            stack.push(name);
            let expanded = self.convert_value(target, stack, needed);
            stack.pop();
            return expanded;
        }

        for_each_subschema(&mut map, |subschema| {
            *subschema = self.convert_value(subschema.take(), stack, needed);
        });
        translate_keywords(map)
    }

    /// Look up a `#/components/schemas/Name` reference, following components
    /// that are themselves only a reference to another.
    ///
    /// Returns the name of the component that holds the schema, so every alias
    /// of one schema shares a single `$defs` entry.
    fn component(&self, reference: &str) -> Option<(&'a str, &'a Schema)> {
        self.component_named(reference.strip_prefix(COMPONENT_PREFIX)?)
    }

    /// [`Self::component`], by component name.
    fn component_named(&self, name: &str) -> Option<(&'a str, &'a Schema)> {
        let spec: &'a OpenAPI = self.spec;
        let schemas = &spec.components.as_ref()?.schemas;
        let mut name = name;
        for _ in 0..MAX_ALIAS_DEPTH {
            let (key, entry) = schemas.get_key_value(name)?;
            match entry {
                ReferenceOr::Item(schema) => return Some((key.as_str(), schema)),
                ReferenceOr::Reference { reference } => {
                    name = reference.strip_prefix(COMPONENT_PREFIX)?;
                }
            }
        }
        // An alias chain that long is a cycle of aliases.
        None
    }
}

/// Move the `$defs` of a nested schema into `defs`, for the enclosing root.
///
/// Definitions are keyed by component name and a component converts the same
/// way wherever it is used, so entries from different properties agree.
pub(crate) fn hoist_defs(schema: &mut Value, defs: &mut Map<String, Value>) {
    if let Some(Value::Object(nested)) = schema.as_object_mut().and_then(|m| m.remove("$defs")) {
        defs.extend(nested);
    }
}

/// Visit every position in a 3.0 Schema Object that holds a schema.
///
/// Walking these, rather than every value, keeps data out of it: a property
/// *named* `example`, or a `default` value that happens to contain `$ref`, is
/// not a keyword.
fn for_each_subschema(schema: &mut Map<String, Value>, mut visit: impl FnMut(&mut Value)) {
    for (keyword, value) in schema.iter_mut() {
        match keyword.as_str() {
            // `additionalProperties` may also be a boolean, which converts to
            // itself.
            "items" | "not" | "additionalProperties" => visit(value),
            "allOf" | "anyOf" | "oneOf" => {
                if let Value::Array(schemas) = value {
                    schemas.iter_mut().for_each(&mut visit);
                }
            }
            "properties" => {
                if let Value::Object(properties) = value {
                    properties.values_mut().for_each(&mut visit);
                }
            }
            _ => {}
        }
    }
}

/// Rewrite one schema's 3.0 keywords as their 2020-12 equivalents.
fn translate_keywords(mut schema: Map<String, Value>) -> Value {
    schema.retain(|keyword, _| {
        !matches!(keyword.as_str(), "discriminator" | "xml" | "externalDocs")
            && !keyword.starts_with("x-")
    });

    for (exclusive, inclusive) in [
        ("exclusiveMinimum", "minimum"),
        ("exclusiveMaximum", "maximum"),
    ] {
        if let Some(&Value::Bool(is_exclusive)) = schema.get(exclusive) {
            // 3.0: a flag on the inclusive bound. 2020-12: the bound itself.
            if is_exclusive && let Some(bound) = schema.remove(inclusive) {
                schema.insert(exclusive.to_string(), bound);
            } else {
                schema.remove(exclusive);
            }
        }
    }

    if let Some(example) = schema.remove("example")
        && !schema.contains_key("examples")
    {
        schema.insert("examples".to_string(), json!([example]));
    }

    if schema.remove("nullable") != Some(Value::Bool(true)) {
        return Value::Object(schema);
    }
    match schema.get_mut("type") {
        Some(Value::String(ty)) => {
            let ty = std::mem::take(ty);
            schema.insert("type".to_string(), json!([ty, "null"]));
            if let Some(Value::Array(values)) = schema.get_mut("enum")
                && !values.contains(&Value::Null)
            {
                values.push(Value::Null);
            }
            Value::Object(schema)
        }
        // 3.0.3 gives `nullable` no effect without a `type`, but specs lean
        // on it beside `allOf: [{$ref}]` to mean "or null". Allowing null is
        // the reading that does not reject values the upstream sends.
        _ => json!({ "anyOf": [Value::Object(schema), { "type": "null" }] }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_spec;

    fn convert_in(spec: Value, schema: Value) -> Value {
        let spec = parse_spec(&spec.to_string()).unwrap();
        let schema: ReferenceOr<Schema> = serde_json::from_value(schema).unwrap();
        SchemaConverter::new(&spec).convert(&schema).unwrap()
    }

    fn convert(schema: Value) -> Value {
        convert_in(
            json!({ "openapi": "3.0.0", "info": { "title": "T", "version": "1" }, "paths": {} }),
            schema,
        )
    }

    fn spec_with_schemas(schemas: Value) -> Value {
        json!({
            "openapi": "3.0.0",
            "info": { "title": "T", "version": "1" },
            "paths": {},
            "components": { "schemas": schemas }
        })
    }

    #[test]
    fn test_nullable_becomes_null_type() {
        assert_eq!(
            convert(json!({ "type": "string", "nullable": true })),
            json!({ "type": ["string", "null"] })
        );
        assert_eq!(
            convert(json!({ "type": "string", "enum": ["a", "b"], "nullable": true })),
            json!({ "type": ["string", "null"], "enum": ["a", "b", null] })
        );
        assert_eq!(
            convert(json!({ "type": "string", "nullable": false })),
            json!({ "type": "string" })
        );
    }

    #[test]
    fn test_nullable_without_type_allows_null() {
        let spec = spec_with_schemas(json!({ "Pet": { "type": "object" } }));
        assert_eq!(
            convert_in(
                spec,
                json!({ "nullable": true, "allOf": [{ "$ref": "#/components/schemas/Pet" }] })
            ),
            json!({ "anyOf": [{ "allOf": [{ "type": "object" }] }, { "type": "null" }] })
        );
    }

    #[test]
    fn test_boolean_exclusive_bounds_become_numeric() {
        assert_eq!(
            convert(json!({
                "type": "number",
                "minimum": 0, "exclusiveMinimum": true,
                "maximum": 10, "exclusiveMaximum": false
            })),
            // openapiv3 holds number bounds as f64.
            json!({ "type": "number", "exclusiveMinimum": 0.0, "maximum": 10.0 })
        );
    }

    #[test]
    fn test_example_becomes_examples_and_openapi_vocabulary_is_dropped() {
        assert_eq!(
            convert(json!({
                "type": "object",
                "example": { "name": "Rex" },
                "discriminator": { "propertyName": "kind" },
                "xml": { "name": "pet" },
                "externalDocs": { "url": "https://example.com" },
                "x-internal": true,
                "properties": {
                    // Property *names* that look like keywords are data.
                    "example": { "type": "integer", "example": 3 },
                    "x-trace": { "type": "string" }
                }
            })),
            json!({
                "type": "object",
                "examples": [{ "name": "Rex" }],
                "properties": {
                    "example": { "type": "integer", "examples": [3] },
                    "x-trace": { "type": "string" }
                }
            })
        );
    }

    #[test]
    fn test_nested_subschemas_are_translated() {
        let converted = convert(json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string", "nullable": true } },
                "extra": {
                    "type": "object",
                    "additionalProperties": { "type": "integer", "nullable": true }
                },
                "either": { "oneOf": [{ "type": "string", "example": "x" }, { "type": "integer" }] }
            }
        }));
        assert_eq!(
            converted.pointer("/properties/tags/items/type"),
            Some(&json!(["string", "null"]))
        );
        assert_eq!(
            converted.pointer("/properties/extra/additionalProperties/type"),
            Some(&json!(["integer", "null"]))
        );
        assert_eq!(
            converted.pointer("/properties/either/oneOf/0/examples"),
            Some(&json!(["x"]))
        );
    }

    #[test]
    fn test_recursive_schema_uses_root_defs() {
        let spec = spec_with_schemas(json!({
            "Node": {
                "type": "object",
                "properties": {
                    "value": { "type": "string" },
                    "children": { "type": "array", "items": { "$ref": "#/components/schemas/Node" } }
                }
            }
        }));
        let converted = convert_in(spec, json!({ "$ref": "#/components/schemas/Node" }));

        // The root is the schema itself, not a bare pointer.
        assert_eq!(converted["type"], "object");
        assert_eq!(
            converted.pointer("/properties/children/items"),
            Some(&json!({ "$ref": "#/$defs/Node" }))
        );
        // And the pointer resolves.
        assert_eq!(
            converted.pointer("/$defs/Node/properties/children/items"),
            Some(&json!({ "$ref": "#/$defs/Node" }))
        );
        assert!(no_dangling_refs(&converted), "{converted:#}");
    }

    #[test]
    fn test_mutual_recursion_through_an_alias_terminates() {
        let spec = spec_with_schemas(json!({
            "Parent": {
                "type": "object",
                "properties": { "child": { "$ref": "#/components/schemas/ChildAlias" } }
            },
            "ChildAlias": { "$ref": "#/components/schemas/Child" },
            "Child": {
                "type": "object",
                "nullable": true,
                "properties": { "parent": { "$ref": "#/components/schemas/Parent" } }
            }
        }));
        let converted = convert_in(spec, json!({ "$ref": "#/components/schemas/Parent" }));

        assert_eq!(
            converted.pointer("/properties/child/type"),
            Some(&json!(["object", "null"]))
        );
        assert_eq!(
            converted.pointer("/properties/child/properties/parent"),
            Some(&json!({ "$ref": "#/$defs/Parent" }))
        );
        assert!(no_dangling_refs(&converted), "{converted:#}");
    }

    #[test]
    fn test_unresolvable_reference_becomes_empty_schema() {
        assert_eq!(
            convert(json!({
                "type": "object",
                "properties": {
                    "missing": { "$ref": "#/components/schemas/Nope" },
                    "external": { "$ref": "other.yaml#/Thing" }
                }
            })),
            json!({ "type": "object", "properties": { "missing": {}, "external": {} } })
        );
    }

    #[test]
    fn test_hoist_defs_moves_nested_defs_to_the_root() {
        let mut property =
            json!({ "$ref": "#/$defs/Node", "$defs": { "Node": { "type": "object" } } });
        let mut defs = Map::new();
        hoist_defs(&mut property, &mut defs);
        assert_eq!(property, json!({ "$ref": "#/$defs/Node" }));
        assert_eq!(Value::Object(defs), json!({ "Node": { "type": "object" } }));
    }

    /// Whether every `$ref` in `schema` points at an entry of its root `$defs`.
    fn no_dangling_refs(schema: &Value) -> bool {
        fn refs<'v>(value: &'v Value, out: &mut Vec<&'v str>) {
            match value {
                Value::Object(map) => {
                    if let Some(Value::String(r)) = map.get("$ref") {
                        out.push(r);
                    }
                    map.values().for_each(|v| refs(v, out));
                }
                Value::Array(items) => items.iter().for_each(|v| refs(v, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        refs(schema, &mut found);
        found.iter().all(|r| {
            r.strip_prefix("#/$defs/")
                .is_some_and(|name| schema.pointer(&format!("/$defs/{name}")).is_some())
        })
    }
}
