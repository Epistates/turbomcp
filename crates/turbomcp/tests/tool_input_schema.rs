//! `#[tool]` input schemas must be self-contained JSON Schema documents.
//!
//! Regression suite for the nested-`$defs` bug: the macro used to run
//! `schema_for!` once per parameter and nest each *root* schema under
//! `properties.<name>`. That moved every `$defs` block one level down while
//! its `#/$defs/...` pointers stayed root-relative, so any parameter whose
//! type needs a definition (`Vec<T>`, `Option<Vec<T>>`, a bare struct with a
//! nested struct) produced dangling references. Validating clients such as
//! llama.cpp reject the whole `tools` array over one such tool.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use turbomcp::prelude::*;

#[derive(Deserialize, JsonSchema)]
struct FrontmatterFilter {
    key: String,
    value: String,
}

#[derive(Deserialize, JsonSchema)]
struct Inner {
    flag: bool,
}

#[derive(Deserialize, JsonSchema)]
struct Outer {
    inner: Inner,
}

/// Self-referential: schemars must emit `#/$defs/Node`, not `"$ref": "#"`.
#[derive(Deserialize, JsonSchema)]
struct Node {
    label: String,
    children: Vec<Node>,
}

mod a {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct Filter {
        pub by_tag: String,
    }
}

mod b {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct Filter {
        pub by_path: String,
    }
}

#[derive(Clone)]
struct SchemaServer;

#[server(name = "schema-server", version = "1.0.0")]
impl SchemaServer {
    /// The shape from the original report: `Option<Vec<Struct>>`.
    #[tool]
    async fn advanced_search(
        &self,
        query: String,
        frontmatter_filters: Option<Vec<FrontmatterFilter>>,
    ) -> String {
        let filters = frontmatter_filters
            .unwrap_or_default()
            .iter()
            .map(|f| format!("{}={}", f.key, f.value))
            .collect::<Vec<_>>();
        format!("{query} {filters:?}")
    }

    /// A bare struct whose field is itself a struct.
    #[tool]
    async fn nested(&self, outer: Outer) -> String {
        format!("{}", outer.inner.flag)
    }

    /// Parameters whose types share a schema name but not a schema. `first`
    /// is inlined; `second` and `third` both need a definition called
    /// `Filter`.
    #[tool]
    async fn same_name(
        &self,
        first: a::Filter,
        second: Vec<a::Filter>,
        third: Vec<b::Filter>,
    ) -> String {
        let tags = second.iter().map(|f| f.by_tag.as_str()).collect::<Vec<_>>();
        let paths = third.iter().map(|f| f.by_path.as_str()).collect::<Vec<_>>();
        format!("{} {tags:?} {paths:?}", first.by_tag)
    }

    /// Two parameters referencing the same definition.
    #[tool]
    async fn shared_def(
        &self,
        include: Vec<FrontmatterFilter>,
        exclude: Vec<FrontmatterFilter>,
    ) -> String {
        format!("{} {}", include.len(), exclude.len())
    }

    #[tool]
    async fn recursive(&self, root: Node) -> String {
        format!("{} ({} children)", root.label, root.children.len())
    }

    #[tool]
    async fn scalars_only(&self, a: i32, b: Option<String>) -> String {
        format!("{a} {b:?}")
    }
}

fn input_schema(tool_name: &str) -> Value {
    let tools = SchemaServer.list_tools();
    let tool = tools
        .iter()
        .find(|t| t.name == tool_name)
        .unwrap_or_else(|| panic!("tool `{tool_name}` not listed"));
    serde_json::to_value(&tool.input_schema).expect("inputSchema serializable")
}

/// Every `$ref` in `value` must resolve as a JSON Pointer against `root`.
fn assert_refs_resolve(root: &Value, value: &Value, path: &str) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                let pointer = r
                    .strip_prefix('#')
                    .unwrap_or_else(|| panic!("{path}: non-local $ref `{r}`"));
                assert!(
                    root.pointer(pointer).is_some(),
                    "{path}: `$ref: {r}` does not resolve against the schema root:\n{}",
                    serde_json::to_string_pretty(root).unwrap()
                );
            }
            for (k, v) in map {
                assert_refs_resolve(root, v, &format!("{path}/{k}"));
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                assert_refs_resolve(root, v, &format!("{path}/{i}"));
            }
        }
        _ => {}
    }
}

/// Only the document root may declare `$schema`.
fn assert_no_nested_dialect(value: &Value, path: &str) {
    match value {
        Value::Object(map) => {
            assert!(
                !map.contains_key("$schema"),
                "{path}: subschema declares `$schema`"
            );
            for (k, v) in map {
                assert_no_nested_dialect(v, &format!("{path}/{k}"));
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                assert_no_nested_dialect(v, &format!("{path}/{i}"));
            }
        }
        _ => {}
    }
}

/// The three invariants from the bug report, applied to a whole tool schema.
fn assert_well_formed(schema: &Value) {
    assert_eq!(
        schema.get("$schema").and_then(Value::as_str),
        Some("https://json-schema.org/draft/2020-12/schema"),
        "root must declare the 2020-12 dialect"
    );
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("properties object");
    for (name, prop) in properties {
        assert_no_nested_dialect(prop, &format!("properties/{name}"));
        // None of the test types declare a title; the only way one shows up
        // is the macro leaking schemars' root `title` (a Rust type name).
        assert!(
            prop.get("title").is_none(),
            "properties/{name}: carries a generated `title`: {prop}"
        );
        assert!(
            prop.get("$defs").is_none(),
            "properties/{name}: `$defs` nested inside a property: {prop}"
        );
    }
    assert_refs_resolve(schema, schema, "");
}

#[test]
fn option_vec_struct_hoists_defs_to_root() {
    let schema = input_schema("advanced_search");
    assert_well_formed(&schema);

    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .expect("root $defs");
    assert!(defs.contains_key("FrontmatterFilter"), "defs: {defs:?}");

    let items = schema
        .pointer("/properties/frontmatter_filters/items")
        .expect("items");
    assert_eq!(
        items.get("$ref").and_then(Value::as_str),
        Some("#/$defs/FrontmatterFilter")
    );
}

#[test]
fn bare_struct_stays_inline_and_nested_defs_hoist() {
    let schema = input_schema("nested");
    assert_well_formed(&schema);

    // The parameter's own type is inlined (unchanged from before the fix);
    // only the type it references moves to `$defs`.
    assert_eq!(
        schema
            .pointer("/properties/outer/type")
            .and_then(Value::as_str),
        Some("object")
    );
    assert_eq!(
        schema
            .pointer("/properties/outer/properties/inner/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/Inner")
    );
    assert!(schema.pointer("/$defs/Inner").is_some());
}

#[test]
fn colliding_definition_names_are_disambiguated() {
    let schema = input_schema("same_name");
    assert_well_formed(&schema);

    // `first` is inlined and never claims a definition name.
    assert!(
        schema
            .pointer("/properties/first/properties/by_tag")
            .is_some()
    );

    // `a::Filter` and `b::Filter` both want to be called `Filter`; schemars
    // keys definitions by type path, so the second one gets a distinct name
    // instead of overwriting the first.
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .expect("root $defs");
    assert_eq!(defs.len(), 2, "one definition per distinct type: {defs:?}");

    let definition_for = |param: &str| {
        let r = schema
            .pointer(&format!("/properties/{param}/items/$ref"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{param}.items.$ref"));
        schema
            .pointer(r.trim_start_matches('#'))
            .unwrap_or_else(|| panic!("{param}: `{r}` resolves"))
    };
    assert!(
        definition_for("second")
            .pointer("/properties/by_tag")
            .is_some(),
        "`second` must point at a::Filter"
    );
    assert!(
        definition_for("third")
            .pointer("/properties/by_path")
            .is_some(),
        "`third` must point at b::Filter"
    );
}

#[test]
fn shared_type_yields_one_definition() {
    let schema = input_schema("shared_def");
    assert_well_formed(&schema);

    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .expect("root $defs");
    assert_eq!(
        defs.len(),
        1,
        "one definition shared by both params: {defs:?}"
    );
    for name in ["include", "exclude"] {
        assert_eq!(
            schema
                .pointer(&format!("/properties/{name}/items/$ref"))
                .and_then(Value::as_str),
            Some("#/$defs/FrontmatterFilter")
        );
    }
}

#[test]
fn recursive_type_references_a_root_definition() {
    let schema = input_schema("recursive");
    assert_well_formed(&schema);

    // `"$ref": "#"` would point at the tool schema, not at `Node`.
    assert_eq!(
        schema
            .pointer("/properties/root/properties/children/items/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/Node")
    );
}

#[test]
fn scalar_only_tool_has_no_defs() {
    let schema = input_schema("scalars_only");
    assert_well_formed(&schema);
    assert!(
        schema.get("$defs").is_none(),
        "no definitions were generated, so `$defs` must be absent: {schema}"
    );
    assert_eq!(
        schema.pointer("/properties/a/type").and_then(Value::as_str),
        Some("integer")
    );
}
