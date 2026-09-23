//! Every `requestedSchema` shape MCP 2025-11-25 permits, round-tripped.
//!
//! The typed view used to cover only string / number / integer / boolean:
//! a multi-select property failed the *whole* `ElicitationSchema` with
//! `unknown variant 'array'`, and a titled single-select parsed but dropped its
//! `oneOf` on the way back out. Both are silent from the client's side — the
//! typed accessor returns `None`, so a UI renders a free-text box where the
//! server asked for a dropdown, or declines outright.
//!
//! The four JSON blocks below are the spec's own examples from
//! client/elicitation.mdx §Requested Schema item 4, verbatim.

use serde_json::json;
use turbomcp_types::{ElicitationSchema, EnumOption, MultiSelectItemsDefinition};

/// Parse a property through `ElicitationSchema` — not standalone — and assert
/// it comes back byte-identical. Going through `properties` is the point: that
/// is the path the client's typed accessor takes, and the one that was broken.
fn round_trips(property: serde_json::Value) {
    let schema = json!({
        "type": "object",
        "properties": { "field": property },
        "required": ["field"],
    });

    let parsed: ElicitationSchema = serde_json::from_value(schema.clone())
        .unwrap_or_else(|e| panic!("schema should parse: {e}\n{schema:#}"));
    let reserialized = serde_json::to_value(&parsed).expect("schema should serialize");

    assert_eq!(
        reserialized["properties"]["field"], schema["properties"]["field"],
        "the property must survive the round trip unchanged"
    );
}

#[test]
fn single_select_without_titles_round_trips() {
    round_trips(json!({
        "type": "string",
        "title": "Color Selection",
        "description": "Choose your favorite color",
        "enum": ["Red", "Green", "Blue"],
        "default": "Red"
    }));
}

#[test]
fn single_select_with_titles_round_trips() {
    round_trips(json!({
        "type": "string",
        "title": "Color Selection",
        "description": "Choose your favorite color",
        "oneOf": [
            { "const": "#FF0000", "title": "Red" },
            { "const": "#00FF00", "title": "Green" },
            { "const": "#0000FF", "title": "Blue" }
        ],
        "default": "#FF0000"
    }));
}

#[test]
fn multi_select_without_titles_round_trips() {
    round_trips(json!({
        "type": "array",
        "title": "Color Selection",
        "description": "Choose your favorite colors",
        "minItems": 1,
        "maxItems": 2,
        "items": {
            "type": "string",
            "enum": ["Red", "Green", "Blue"]
        },
        "default": ["Red", "Green"]
    }));
}

#[test]
fn multi_select_with_titles_round_trips() {
    round_trips(json!({
        "type": "array",
        "title": "Color Selection",
        "description": "Choose your favorite colors",
        "minItems": 1,
        "maxItems": 2,
        "items": {
            "anyOf": [
                { "const": "#FF0000", "title": "Red" },
                { "const": "#00FF00", "title": "Green" },
                { "const": "#0000FF", "title": "Blue" }
            ]
        },
        "default": ["#FF0000", "#00FF00"]
    }));
}

/// The plain shapes have to keep working — this is an additive change.
#[test]
fn the_primitive_shapes_still_round_trip() {
    for property in [
        json!({ "type": "string", "format": "email", "minLength": 3 }),
        json!({ "type": "number", "minimum": 0.0, "maximum": 1.0 }),
        json!({ "type": "integer", "minimum": 1, "default": 7 }),
        json!({ "type": "boolean", "default": true }),
    ] {
        round_trips(property);
    }
}

/// A titled multi-select must not be read as an untitled one. `anyOf` is the
/// more specific shape, so the untagged enum has to try it first.
#[test]
fn titled_items_are_not_mistaken_for_untitled_items() {
    let schema: ElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "colors": {
                "type": "array",
                "items": { "anyOf": [{ "const": "r", "title": "Red" }] }
            }
        }
    }))
    .expect("schema parses");

    let turbomcp_types::PrimitiveSchemaDefinition::Array { items, .. } =
        &schema.properties["colors"]
    else {
        panic!("expected an array property");
    };
    assert!(
        matches!(items, MultiSelectItemsDefinition::Titled(_)),
        "anyOf items carry titles and must parse as the titled form"
    );
}

/// The server-side builders produce the same shapes, so a server does not have
/// to hand-assemble JSON to offer a dropdown.
#[test]
fn the_builders_produce_spec_shapes() {
    let schema = ElicitationSchema::new()
        .add_enum_property(
            "color".into(),
            true,
            Some("Pick one".into()),
            vec![EnumOption {
                const_value: "#FF0000".into(),
                title: "Red".into(),
            }],
        )
        .add_multi_select_property(
            "tags".into(),
            false,
            None,
            MultiSelectItemsDefinition::Untitled(turbomcp_types::UntitledMultiSelectItems {
                schema_type: "string".into(),
                enum_values: vec!["a".into(), "b".into()],
            }),
        );

    let json = serde_json::to_value(&schema).expect("serializes");
    assert_eq!(json["properties"]["color"]["type"], "string");
    assert_eq!(json["properties"]["color"]["oneOf"][0]["const"], "#FF0000");
    assert_eq!(json["properties"]["tags"]["type"], "array");
    assert_eq!(json["properties"]["tags"]["items"]["enum"][1], "b");
    assert_eq!(json["required"], json!(["color"]));
}

/// Python's `json.dumps` writes `50.0` for an integral float. The schema types
/// these bounds as numbers, so refusing them failed the whole typed schema and
/// a client's UI fell back to nothing.
#[test]
fn integral_numbers_written_with_a_fraction_parse() {
    let schema: ElicitationSchema = serde_json::from_value(json!({
        "type": "object",
        "properties": { "n": { "type": "integer", "minimum": 1.0, "default": 50.0 } }
    }))
    .expect("an integral 50.0 is an integer");
    assert!(matches!(
        schema.properties["n"],
        turbomcp_types::PrimitiveSchemaDefinition::Integer {
            minimum: Some(1),
            default: Some(50),
            ..
        }
    ));
}

/// The legacy `enum` + `enumNames` form is deprecated but still in the union;
/// read as a plain enum it lost the names the user is shown.
#[test]
fn the_legacy_titled_enum_keeps_its_names() {
    let parsed: turbomcp_types::EnumSchema = serde_json::from_value(json!({
        "type": "string",
        "enum": ["r", "g"],
        "enumNames": ["Red", "Green"]
    }))
    .expect("parses");
    assert!(
        matches!(
            parsed,
            turbomcp_types::EnumSchema::LegacyTitledSingleSelect(ref legacy)
                if legacy.enum_names == ["Red", "Green"]
        ),
        "{parsed:?}"
    );
}
