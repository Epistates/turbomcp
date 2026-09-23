//! Elicitation input schema types for MCP 2025-11-25.
//!
//! These types describe the JSON Schema a server sends when it wants a client
//! to gather structured input from the user (via `elicitation/create`). They
//! are the Rust surface for the `requestedSchema` field on form-mode
//! elicitation requests.
//!
//! ## Layers
//!
//! - [`ElicitationSchema`] — top-level object schema (`{ type: "object", properties, required, additionalProperties }`)
//! - [`PrimitiveSchemaDefinition`] — per-field schema (String / Number / Integer / Boolean)
//! - [`EnumSchema`] and friends (SEP-1330) — standards-based enum patterns using
//!   `oneOf` / `anyOf` / `const` / `enum` keywords from JSON Schema 2020-12.
//!
//! These types are no_std-compatible; on `no_std + alloc` builds the internal
//! map is `alloc::collections::BTreeMap`.
//!
//! [`URLElicitationRequiredError`] carries the URL payload servers return when
//! they need the client to switch to URL-mode elicitation.

use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use alloc::{
    collections::BTreeMap as HashMap,
    string::{String, ToString},
    vec::Vec,
};
#[cfg(feature = "std")]
use std::collections::HashMap;

// =============================================================================
// Elicitation form schema (requestedSchema)
// =============================================================================

/// Top-level object schema for a form-mode elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElicitationSchema {
    /// Schema type — must be `"object"` per MCP spec.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Per-field schemas keyed by property name.
    pub properties: HashMap<String, PrimitiveSchemaDefinition>,
    /// Names of required properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional (unspecified) properties are allowed.
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

impl ElicitationSchema {
    /// Create an empty object schema with `required: []` and `additionalProperties: false`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: HashMap::new(),
            required: Some(Vec::new()),
            additional_properties: Some(false),
        }
    }

    /// Add a string property.
    #[must_use]
    pub fn add_string_property(
        mut self,
        name: String,
        required: bool,
        description: Option<String>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::String {
            title: None,
            description,
            format: None,
            min_length: None,
            max_length: None,
            default: None,
            enum_values: None,
            enum_names: None,
            one_of: None,
        };
        self.properties.insert(name.clone(), property);
        if required && let Some(required_fields) = self.required.as_mut() {
            required_fields.push(name);
        }
        self
    }

    /// Add a single-select property rendered as a dropdown.
    ///
    /// Each option carries its own display title (SEP-1330 `oneOf` + `const`),
    /// which is why this is preferred over the legacy `enum` + `enumNames`
    /// pairing: a client never has to line two parallel arrays up itself.
    #[must_use]
    pub fn add_enum_property(
        mut self,
        name: String,
        required: bool,
        description: Option<String>,
        options: Vec<EnumOption>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::String {
            title: None,
            description,
            format: None,
            min_length: None,
            max_length: None,
            default: None,
            enum_values: None,
            enum_names: None,
            one_of: Some(options),
        };
        self.properties.insert(name.clone(), property);
        if required && let Some(required_fields) = self.required.as_mut() {
            required_fields.push(name);
        }
        self
    }

    /// Add a multi-select property.
    #[must_use]
    pub fn add_multi_select_property(
        mut self,
        name: String,
        required: bool,
        description: Option<String>,
        items: MultiSelectItemsDefinition,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::Array {
            title: None,
            description,
            min_items: None,
            max_items: None,
            items,
            default: None,
        };
        self.properties.insert(name.clone(), property);
        if required && let Some(required_fields) = self.required.as_mut() {
            required_fields.push(name);
        }
        self
    }

    /// Add a number property.
    #[must_use]
    pub fn add_number_property(
        mut self,
        name: String,
        required: bool,
        description: Option<String>,
        minimum: Option<f64>,
        maximum: Option<f64>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::Number {
            title: None,
            description,
            minimum,
            maximum,
            default: None,
        };
        self.properties.insert(name.clone(), property);
        if required && let Some(required_fields) = self.required.as_mut() {
            required_fields.push(name);
        }
        self
    }

    /// Add a boolean property.
    #[must_use]
    pub fn add_boolean_property(
        mut self,
        name: String,
        required: bool,
        description: Option<String>,
        default: Option<bool>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::Boolean {
            title: None,
            description,
            default,
        };
        self.properties.insert(name.clone(), property);
        if required && let Some(required_fields) = self.required.as_mut() {
            required_fields.push(name);
        }
        self
    }
}

impl Default for ElicitationSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-field schema for an [`ElicitationSchema`].
///
/// Covers every shape MCP 2025-11-25 allows in `requestedSchema.properties`:
/// String / Number / Integer / Boolean, the two single-select enum forms
/// (`enum`, or `oneOf` with titles), and the two multi-select forms as
/// [`Self::Array`]. For enums prefer `one_of` (SEP-1330) over the legacy
/// `enum_values` / `enum_names` pattern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PrimitiveSchemaDefinition {
    /// String-valued field.
    #[serde(rename = "string")]
    String {
        /// Optional human-readable title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// JSON Schema `format` (email, uri, date-time, …).
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        /// Minimum string length.
        #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
        min_length: Option<u32>,
        /// Maximum string length.
        #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
        /// Default value (MCP 2025-11-25 spec).
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        /// Legacy enum values (prefer [`EnumSchema::UntitledSingleSelect`]).
        #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
        enum_values: Option<Vec<String>>,
        /// Legacy display names for `enum_values` (deprecated; prefer
        /// [`Self::String::one_of`]).
        #[serde(rename = "enumNames", skip_serializing_if = "Option::is_none")]
        enum_names: Option<Vec<String>>,
        /// Titled single-select options (`oneOf` + `const`), SEP-1330.
        ///
        /// The preferred way to offer a dropdown: each option carries its own
        /// display title, so a client does not have to pair two parallel
        /// arrays the way `enum` + `enumNames` requires.
        #[serde(rename = "oneOf", skip_serializing_if = "Option::is_none")]
        one_of: Option<Vec<EnumOption>>,
    },
    /// Number-valued field.
    #[serde(rename = "number")]
    Number {
        /// Optional human-readable title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum value.
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        /// Maximum value.
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
        /// Default value (MCP 2025-11-25 spec).
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    /// Integer-valued field.
    #[serde(rename = "integer")]
    Integer {
        /// Optional human-readable title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum value.
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<i64>,
        /// Maximum value.
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<i64>,
        /// Default value (MCP 2025-11-25 spec).
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
    },
    /// Boolean-valued field.
    #[serde(rename = "boolean")]
    Boolean {
        /// Optional human-readable title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Default value.
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
    /// Multi-select field (SEP-1330): an array of values drawn from a fixed set.
    ///
    /// Without this variant the whole `ElicitationSchema` failed to deserialize
    /// with `unknown variant 'array'` — so a client following the typed path
    /// saw no schema at all for a request the spec fully permits.
    #[serde(rename = "array")]
    Array {
        /// Optional human-readable title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum number of selections.
        #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
        min_items: Option<u32>,
        /// Maximum number of selections.
        #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
        max_items: Option<u32>,
        /// The allowed options, titled (`anyOf`) or plain (`enum`).
        items: MultiSelectItemsDefinition,
        /// Optional default selection.
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<Vec<String>>,
    },
}

/// The two item shapes a multi-select may take.
///
/// Untagged, with the titled form first: `anyOf` is the more specific shape, so
/// trying it first stops a titled schema being read as an untitled one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MultiSelectItemsDefinition {
    /// `{ "anyOf": [{ "const": …, "title": … }] }`
    Titled(MultiSelectItems),
    /// `{ "type": "string", "enum": [ … ] }`
    Untitled(UntitledMultiSelectItems),
}

// =============================================================================
// SEP-1330: Standards-based enum schemas
// =============================================================================

/// A single enum option with a value and display title (JSON Schema 2020-12).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumOption {
    /// The allowed value.
    #[serde(rename = "const")]
    pub const_value: String,
    /// Human-readable label for the value.
    pub title: String,
}

/// Single-select enum schema with titles (`oneOf` + `const`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TitledSingleSelectEnumSchema {
    /// Schema type — must be `"string"`.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// The list of allowed `{ const, title }` options.
    #[serde(rename = "oneOf")]
    pub one_of: Vec<EnumOption>,
    /// Optional title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value (must match one of the `const` values).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Single-select enum schema without titles (plain `enum`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UntitledSingleSelectEnumSchema {
    /// Schema type — must be `"string"`.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// The allowed values.
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
    /// Optional title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Multi-select enum schema with titles (`array` + `anyOf`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TitledMultiSelectEnumSchema {
    /// Schema type — must be `"array"`.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Minimum number of selections.
    #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Maximum number of selections.
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    /// Item schema using `anyOf`.
    pub items: MultiSelectItems,
    /// Optional title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default (array of chosen values).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
}

/// Multi-select enum schema without titles (`array` + `enum`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UntitledMultiSelectEnumSchema {
    /// Schema type — must be `"array"`.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Minimum number of selections.
    #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Maximum number of selections.
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    /// Item schema using `enum`.
    pub items: UntitledMultiSelectItems,
    /// Optional title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default (array of chosen values).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
}

/// Item schema for [`TitledMultiSelectEnumSchema`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultiSelectItems {
    /// Allowed `{ const, title }` options.
    #[serde(rename = "anyOf")]
    pub any_of: Vec<EnumOption>,
}

/// Item schema for [`UntitledMultiSelectEnumSchema`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UntitledMultiSelectItems {
    /// Item type — must be `"string"`.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Allowed values.
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
}

/// Union of standards-based enum schema variants (SEP-1330).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum EnumSchema {
    /// Single-select enum with titles (`oneOf` + `const`).
    TitledSingleSelect(TitledSingleSelectEnumSchema),
    /// Single-select enum without titles (plain `enum`).
    UntitledSingleSelect(UntitledSingleSelectEnumSchema),
    /// Multi-select enum with titles (`array` + `anyOf`).
    TitledMultiSelect(TitledMultiSelectEnumSchema),
    /// Multi-select enum without titles (`array` + `enum`).
    UntitledMultiSelect(UntitledMultiSelectEnumSchema),
}

// =============================================================================
// URL elicitation required error payload
// =============================================================================

/// `data` payload of a `-32042` error telling the client that one or more URL
/// mode elicitations must complete before the request can be retried.
///
/// Carry this as the `data` member of a JSON-RPC error with code
/// [`Self::ERROR_CODE`], per MCP 2025-11-25 / SEP-1036. The spec types `data`
/// as an object with a **required `elicitations` array**, and requires every
/// entry to be a URL mode elicitation carrying an `elicitationId` — which is
/// what lets the client correlate the later
/// `notifications/elicitation/complete` and retry.
///
/// ```rust
/// use turbomcp_types::{URLElicitationRequiredError, ElicitRequestURLParams};
///
/// let payload = URLElicitationRequiredError::single(ElicitRequestURLParams {
///     message: "Authorize access to your files.".into(),
///     url: "https://example.com/connect?e=550e8400".into(),
///     elicitation_id: "550e8400".into(),
///     task: None,
///     meta: None,
/// });
/// let json = serde_json::to_value(&payload).unwrap();
/// assert_eq!(json["elicitations"][0]["mode"], "url");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct URLElicitationRequiredError {
    /// The elicitations that must complete before the original request can be
    /// retried. Required, and every entry is a URL mode elicitation.
    ///
    /// Each entry is written with `"mode": "url"`: the schema types these as
    /// `ElicitRequestURLParams`, where `mode` is required, and the bare struct
    /// has no field for it — only [`crate::protocol::ElicitRequestParams`]
    /// adds it.
    #[serde(serialize_with = "serialize_url_elicitations")]
    pub elicitations: Vec<crate::protocol::ElicitRequestURLParams>,
}

fn serialize_url_elicitations<S: serde::Serializer>(
    elicitations: &[crate::protocol::ElicitRequestURLParams],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;

    let mut seq = serializer.serialize_seq(Some(elicitations.len()))?;
    for elicitation in elicitations {
        seq.serialize_element(&crate::protocol::ElicitRequestParams::Url(
            elicitation.clone(),
        ))?;
    }
    seq.end()
}

impl URLElicitationRequiredError {
    /// JSON-RPC error code for URL-elicitation-required.
    pub const ERROR_CODE: i32 = -32042;

    /// Require a single elicitation, the common case.
    #[must_use]
    pub fn single(elicitation: crate::protocol::ElicitRequestURLParams) -> Self {
        Self {
            elicitations: vec![elicitation],
        }
    }

    /// Require several elicitations to complete before retrying.
    #[must_use]
    pub fn new(
        elicitations: impl IntoIterator<Item = crate::protocol::ElicitRequestURLParams>,
    ) -> Self {
        Self {
            elicitations: elicitations.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn elicitation_schema_builder_round_trip() {
        let schema = ElicitationSchema::new()
            .add_string_property("name".into(), true, Some("User name".into()))
            .add_number_property("age".into(), false, None, Some(0.0), Some(120.0));
        let json = serde_json::to_string(&schema).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "object");
        assert!(v["properties"]["name"].is_object());
        assert_eq!(v["properties"]["name"]["type"], "string");
        assert_eq!(v["properties"]["age"]["type"], "number");
        assert_eq!(
            v["required"].as_array().unwrap(),
            &vec![Value::from("name")]
        );
        assert_eq!(v["additionalProperties"], false);
    }

    #[test]
    fn primitive_schema_string_serde() {
        let s = PrimitiveSchemaDefinition::String {
            title: Some("Name".into()),
            description: None,
            format: Some("email".into()),
            min_length: Some(1),
            max_length: Some(80),
            default: None,
            enum_values: None,
            enum_names: None,
            one_of: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"string\""));
        assert!(json.contains("\"format\":\"email\""));
        let back: PrimitiveSchemaDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn titled_single_select_enum_schema_round_trip() {
        let schema = TitledSingleSelectEnumSchema {
            schema_type: "string".into(),
            one_of: vec![
                EnumOption {
                    const_value: "#FF0000".into(),
                    title: "Red".into(),
                },
                EnumOption {
                    const_value: "#00FF00".into(),
                    title: "Green".into(),
                },
            ],
            title: Some("Color".into()),
            description: None,
            default: Some("#FF0000".into()),
        };
        let json = serde_json::to_string(&schema).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "string");
        assert!(v["oneOf"].is_array());
        assert_eq!(v["default"], "#FF0000");
        let back: TitledSingleSelectEnumSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, back);
    }

    #[test]
    fn enum_schema_union_discriminates_correctly() {
        let titled = r#"{"type":"string","oneOf":[{"const":"a","title":"A"}]}"#;
        match serde_json::from_str::<EnumSchema>(titled).unwrap() {
            EnumSchema::TitledSingleSelect(_) => {}
            _ => panic!("expected TitledSingleSelect"),
        }
        let untitled = r#"{"type":"string","enum":["a","b"]}"#;
        match serde_json::from_str::<EnumSchema>(untitled).unwrap() {
            EnumSchema::UntitledSingleSelect(_) => {}
            _ => panic!("expected UntitledSingleSelect"),
        }
        let multi_titled = r#"{"type":"array","items":{"anyOf":[{"const":"a","title":"A"}]}}"#;
        match serde_json::from_str::<EnumSchema>(multi_titled).unwrap() {
            EnumSchema::TitledMultiSelect(_) => {}
            _ => panic!("expected TitledMultiSelect"),
        }
        let multi_untitled = r#"{"type":"array","items":{"type":"string","enum":["a","b"]}}"#;
        match serde_json::from_str::<EnumSchema>(multi_untitled).unwrap() {
            EnumSchema::UntitledMultiSelect(_) => {}
            _ => panic!("expected UntitledMultiSelect"),
        }
    }

    fn url_params(id: &str) -> crate::protocol::ElicitRequestURLParams {
        crate::protocol::ElicitRequestURLParams {
            message: "Please sign in".into(),
            url: "https://example.com/oauth".into(),
            elicitation_id: id.into(),
            task: None,
            meta: None,
        }
    }

    /// The spec types `data` as an object whose `elicitations` array is
    /// REQUIRED; a flat `{url, description}` is unreadable to a conformant
    /// client, which looks for `data.elicitations[]`.
    #[test]
    fn url_elicitation_required_error_round_trip() {
        let err = URLElicitationRequiredError::single(url_params("e-123"));
        let json = serde_json::to_value(&err).unwrap();

        let entries = json["elicitations"]
            .as_array()
            .expect("`elicitations` must be an array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["elicitationId"], "e-123");
        assert_eq!(entries[0]["url"], "https://example.com/oauth");
        assert_eq!(entries[0]["message"], "Please sign in");
        // Required by the schema's `ElicitRequestURLParams`; a client
        // validating against it rejects the whole error without it.
        assert_eq!(entries[0]["mode"], "url");

        let back: URLElicitationRequiredError = serde_json::from_value(json).unwrap();
        assert_eq!(err, back);
        assert_eq!(URLElicitationRequiredError::ERROR_CODE, -32042);
    }

    /// The error may require more than one interaction before a retry.
    #[test]
    fn url_elicitation_required_error_carries_several() {
        let err = URLElicitationRequiredError::new([url_params("a"), url_params("b")]);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["elicitations"].as_array().unwrap().len(), 2);
    }
}
