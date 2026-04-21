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
            enum_values: None,
            enum_names: None,
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
/// MCP 2025-11-25 allows String / Number / Integer / Boolean. For enums,
/// prefer [`EnumSchema`] (SEP-1330) over the legacy `enum_values` / `enum_names`
/// pattern on the `String` variant.
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
        /// Legacy enum values (prefer [`EnumSchema::UntitledSingleSelect`]).
        #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
        enum_values: Option<Vec<String>>,
        /// Legacy display names for `enum_values` (deprecated; prefer
        /// [`EnumSchema::TitledSingleSelect`]).
        #[serde(rename = "enumNames", skip_serializing_if = "Option::is_none")]
        enum_names: Option<Vec<String>>,
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

/// Server-to-client error payload indicating that URL-mode elicitation is
/// required instead of form-mode. Carry this as the `data` field of a
/// JSON-RPC error (code `-32001`) per MCP 2025-11-25 / SEP-1036.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct URLElicitationRequiredError {
    /// The URL the user should open out-of-band.
    pub url: String,
    /// Optional human-readable description of what is being requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional elicitation ID for correlating the follow-up completion
    /// notification.
    #[serde(rename = "elicitationId", skip_serializing_if = "Option::is_none")]
    pub elicitation_id: Option<String>,
}

impl URLElicitationRequiredError {
    /// Create a new URL-elicitation-required error with just the URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            description: None,
            elicitation_id: None,
        }
    }

    /// Attach a human-readable description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Attach a correlation ID for the out-of-band completion notification.
    #[must_use]
    pub fn with_elicitation_id(mut self, id: impl Into<String>) -> Self {
        self.elicitation_id = Some(id.into());
        self
    }

    /// Suggested JSON-RPC error code for URL-elicitation-required.
    pub const ERROR_CODE: i32 = -32001;
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
            enum_values: None,
            enum_names: None,
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

    #[test]
    fn url_elicitation_required_error_round_trip() {
        let err = URLElicitationRequiredError::new("https://example.com/oauth")
            .with_description("Please sign in")
            .with_elicitation_id("e-123");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"elicitationId\":\"e-123\""));
        let back: URLElicitationRequiredError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
        assert_eq!(URLElicitationRequiredError::ERROR_CODE, -32001);
    }
}
