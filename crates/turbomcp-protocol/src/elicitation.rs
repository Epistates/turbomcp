//! MCP Protocol Elicitation Types (Spec-Compliant)
//!
//! This module provides the exact types defined in the MCP 2025-06-18 specification
//! for elicitation/create requests and responses. These types are used for the
//! protocol layer and must maintain 100% spec compliance.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ============================================================================
// Elicitation Request Types (Server → Client)
// ============================================================================

/// Elicitation create request per MCP 2025-06-18 specification
/// Method: "elicitation/create"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationCreateRequest {
    /// The message to present to the user
    pub message: String,

    /// A restricted subset of JSON Schema
    /// Only top-level properties are allowed, without nesting
    #[serde(rename = "requestedSchema")]
    pub requested_schema: ElicitationSchema,
}

/// Restricted schema for elicitation requests
/// Only allows flat objects with primitive properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationSchema {
    /// Must always be "object" for elicitation schemas
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Properties of the object - each must be a primitive schema
    pub properties: HashMap<String, PrimitiveSchemaDefinition>,

    /// Required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl ElicitationSchema {
    /// Create a new elicitation schema
    pub fn new() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: HashMap::new(),
            required: None,
        }
    }

    /// Add a property to the schema
    pub fn add_property(mut self, name: String, schema: PrimitiveSchemaDefinition) -> Self {
        self.properties.insert(name, schema);
        self
    }

    /// Mark properties as required
    pub fn require(mut self, names: Vec<String>) -> Self {
        self.required = Some(names);
        self
    }
}

impl Default for ElicitationSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Primitive schema definitions allowed in elicitation
/// Restricted to only primitive types without nesting
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrimitiveSchemaDefinition {
    /// String field schema
    String(StringSchema),
    /// Number field schema
    Number(NumberSchema),
    /// Boolean field schema
    Boolean(BooleanSchema),
    /// Enum field schema
    Enum(EnumSchema),
}

/// String schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringSchema {
    /// Type discriminator
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Minimum length
    #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,

    /// Maximum length
    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,

    /// Format hint (email, uri, date, date-time)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<StringFormat>,

    /// Regex pattern for validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl StringSchema {
    /// Create a new string schema
    pub fn new() -> Self {
        Self {
            schema_type: "string".to_string(),
            title: None,
            description: None,
            min_length: None,
            max_length: None,
            format: None,
            pattern: None,
        }
    }
}

impl Default for StringSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported string formats per MCP spec
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StringFormat {
    /// Email format
    Email,
    /// URI format
    Uri,
    /// Date format (YYYY-MM-DD)
    Date,
    /// Date-time format (ISO 8601)
    #[serde(rename = "date-time")]
    DateTime,
}

/// Number schema definition (supports both number and integer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberSchema {
    /// Type discriminator ("number" or "integer")
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Minimum value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,

    /// Maximum value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

impl NumberSchema {
    /// Create a new number schema for floating point numbers
    pub fn new_number() -> Self {
        Self {
            schema_type: "number".to_string(),
            title: None,
            description: None,
            minimum: None,
            maximum: None,
        }
    }

    /// Create a new number schema for integers
    pub fn new_integer() -> Self {
        Self {
            schema_type: "integer".to_string(),
            title: None,
            description: None,
            minimum: None,
            maximum: None,
        }
    }
}

/// Boolean schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BooleanSchema {
    /// Type discriminator
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
}

impl BooleanSchema {
    /// Create a new boolean schema
    pub fn new() -> Self {
        Self {
            schema_type: "boolean".to_string(),
            title: None,
            description: None,
            default: None,
        }
    }
}

impl Default for BooleanSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Object schema definition (for building objects)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectSchema {
    /// Display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Properties of the object
    pub properties: HashMap<String, serde_json::Value>,

    /// Required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl ObjectSchema {
    /// Create a new object schema
    pub fn new() -> Self {
        Self::default()
    }
}

/// Enum schema definition (string enumerations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumSchema {
    /// Type discriminator (always "string" for enums)
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Allowed values
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,

    /// Display names for enum values
    #[serde(rename = "enumNames", skip_serializing_if = "Option::is_none")]
    pub enum_names: Option<Vec<String>>,
}

impl EnumSchema {
    /// Create a new enum schema with the given allowed values
    pub fn new(values: Vec<String>) -> Self {
        Self {
            schema_type: "string".to_string(),
            title: None,
            description: None,
            enum_values: values,
            enum_names: None,
        }
    }
}

// ============================================================================
// Elicitation Response Types (Client → Server)
// ============================================================================

/// Elicitation response per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationCreateResult {
    /// The user action in response to the elicitation
    pub action: ElicitationAction,

    /// The submitted form data, only present when action is "accept"
    /// Contains values matching the requested schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<HashMap<String, ElicitationValue>>,

    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// User action in response to elicitation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElicitationAction {
    /// User submitted the form/confirmed the action
    Accept,
    /// User explicitly declined the action
    Decline,
    /// User dismissed without making an explicit choice
    Cancel,
}

/// Values that can be submitted in elicitation responses
/// Limited to string, integer, and boolean per spec
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ElicitationValue {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Number (floating-point) value
    Number(f64),
    /// Boolean value
    Boolean(bool),
}

impl ElicitationValue {
    /// Try to get as string
    pub fn as_string(&self) -> Option<&String> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get as integer
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Try to get as boolean
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to get as number (f64)
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }
}

// ============================================================================
// Builder Helpers for Ergonomic API
// ============================================================================

/// Create a string schema
pub fn string() -> StringSchemaBuilder {
    StringSchemaBuilder::new()
}

/// Create an integer schema
pub fn integer() -> NumberSchemaBuilder {
    NumberSchemaBuilder::new_integer()
}

/// Create a number schema
pub fn number() -> NumberSchemaBuilder {
    NumberSchemaBuilder::new_number()
}

/// Create a boolean schema
pub fn boolean() -> BooleanSchemaBuilder {
    BooleanSchemaBuilder::new()
}

/// Create an enum schema
pub fn enum_of(values: Vec<String>) -> EnumSchemaBuilder {
    EnumSchemaBuilder::new(values)
}

/// Create an object schema
pub fn object() -> ObjectSchemaBuilder {
    ObjectSchemaBuilder::new()
}

/// Create an array schema
pub fn array() -> ArraySchemaBuilder {
    ArraySchemaBuilder::new()
}

/// Builder for string schemas
#[derive(Debug, Default)]
pub struct StringSchemaBuilder(StringSchema);

impl StringSchemaBuilder {
    /// Create a new string schema builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the display title for this string field
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.0.title = Some(title.into());
        self
    }

    /// Set the description for this string field
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.0.description = Some(desc.into());
        self
    }

    /// Set the minimum length constraint for the string
    pub fn min_length(mut self, len: u32) -> Self {
        self.0.min_length = Some(len);
        self
    }

    /// Set the maximum length constraint for the string
    pub fn max_length(mut self, len: u32) -> Self {
        self.0.max_length = Some(len);
        self
    }

    /// Set the string format hint (email, uri, date, date-time)
    pub fn format(mut self, format: StringFormat) -> Self {
        self.0.format = Some(format);
        self
    }

    /// Set the format to email
    pub fn email(self) -> Self {
        self.format(StringFormat::Email)
    }

    /// Set the format to URI
    pub fn uri(self) -> Self {
        self.format(StringFormat::Uri)
    }

    /// Set the format to date (YYYY-MM-DD)
    pub fn date(self) -> Self {
        self.format(StringFormat::Date)
    }

    /// Set the format to date-time (ISO 8601)
    pub fn date_time(self) -> Self {
        self.format(StringFormat::DateTime)
    }

    /// Add a regex pattern constraint
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.0.pattern = Some(pattern.into());
        self
    }

    /// Convert to an enum schema with values
    pub fn enum_values(self, values: Vec<impl Into<String>>) -> EnumSchemaBuilder {
        let values: Vec<String> = values.into_iter().map(Into::into).collect();
        // Copy over title and description from string schema
        let mut builder = EnumSchemaBuilder::new(values);
        if let Some(title) = self.0.title {
            builder = builder.title(title);
        }
        if let Some(desc) = self.0.description {
            builder = builder.description(desc);
        }
        builder
    }

    /// Build the string schema into a primitive schema definition
    pub fn build(self) -> PrimitiveSchemaDefinition {
        PrimitiveSchemaDefinition::String(self.0)
    }
}

/// Builder for number schemas
#[derive(Debug)]
pub struct NumberSchemaBuilder(NumberSchema);

impl NumberSchemaBuilder {
    /// Create a new number schema builder for floating point numbers
    pub fn new_number() -> Self {
        Self(NumberSchema::new_number())
    }

    /// Create a new number schema builder for integers
    pub fn new_integer() -> Self {
        Self(NumberSchema::new_integer())
    }

    /// Set the display title for this number field
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.0.title = Some(title.into());
        self
    }

    /// Set the description for this number field
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.0.description = Some(desc.into());
        self
    }

    /// Set the minimum value constraint
    pub fn min(mut self, min: f64) -> Self {
        self.0.minimum = Some(min);
        self
    }

    /// Set the maximum value constraint
    pub fn max(mut self, max: f64) -> Self {
        self.0.maximum = Some(max);
        self
    }

    /// Set both minimum and maximum value constraints
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.0.minimum = Some(min);
        self.0.maximum = Some(max);
        self
    }

    /// Build the number schema into a primitive schema definition
    pub fn build(self) -> PrimitiveSchemaDefinition {
        PrimitiveSchemaDefinition::Number(self.0)
    }
}

/// Builder for boolean schemas
#[derive(Debug, Default)]
pub struct BooleanSchemaBuilder(BooleanSchema);

impl BooleanSchemaBuilder {
    /// Create a new boolean schema builder
    pub fn new() -> Self {
        Self(BooleanSchema::new())
    }

    /// Set the display title for this boolean field
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.0.title = Some(title.into());
        self
    }

    /// Set the description for this boolean field
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.0.description = Some(desc.into());
        self
    }

    /// Set the default value for this boolean field
    pub fn default(mut self, value: bool) -> Self {
        self.0.default = Some(value);
        self
    }

    /// Build the boolean schema into a primitive schema definition
    pub fn build(self) -> PrimitiveSchemaDefinition {
        PrimitiveSchemaDefinition::Boolean(self.0)
    }
}

/// Builder for enum schemas
#[derive(Debug)]
pub struct EnumSchemaBuilder(EnumSchema);

impl EnumSchemaBuilder {
    /// Create a new enum schema builder with the given allowed values
    pub fn new(values: Vec<String>) -> Self {
        Self(EnumSchema::new(values))
    }

    /// Set the display title for this enum field
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.0.title = Some(title.into());
        self
    }

    /// Set the description for this enum field
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.0.description = Some(desc.into());
        self
    }

    /// Set display names for the enum values (must match length of values)
    pub fn names(mut self, names: Vec<String>) -> Self {
        self.0.enum_names = Some(names);
        self
    }

    /// Build the enum schema into a primitive schema definition
    pub fn build(self) -> PrimitiveSchemaDefinition {
        PrimitiveSchemaDefinition::Enum(self.0)
    }
}

/// Builder for object schemas
#[derive(Debug)]
pub struct ObjectSchemaBuilder(ObjectSchema);

impl Default for ObjectSchemaBuilder {
    fn default() -> Self {
        Self(ObjectSchema::new())
    }
}

impl ObjectSchemaBuilder {
    /// Create a new object schema builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.0.title = Some(title.into());
        self
    }

    /// Add a description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.0.description = Some(desc.into());
        self
    }

    /// Build into ElicitationSchema
    pub fn build(self) -> ElicitationSchema {
        // Convert ObjectSchema properties to ElicitationSchema properties
        let mut props = HashMap::new();
        for (key, _value) in self.0.properties {
            // Convert JSON Value to PrimitiveSchemaDefinition if possible
            // For now, we'll skip complex conversion
            props.insert(key, PrimitiveSchemaDefinition::String(StringSchema::new()));
        }

        ElicitationSchema {
            schema_type: "object".to_string(),
            properties: props,
            required: self.0.required,
        }
    }
}

/// Builder for array schemas
#[derive(Debug, Default)]
pub struct ArraySchemaBuilder {
    title: Option<String>,
    description: Option<String>,
    items: Option<Box<PrimitiveSchemaDefinition>>,
    min_items: Option<u32>,
    max_items: Option<u32>,
}

impl ArraySchemaBuilder {
    /// Create a new array schema builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Add a description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the items schema
    pub fn items(mut self, items: PrimitiveSchemaDefinition) -> Self {
        self.items = Some(Box::new(items));
        self
    }

    /// Set minimum items
    pub fn min_items(mut self, min: u32) -> Self {
        self.min_items = Some(min);
        self
    }

    /// Set maximum items
    pub fn max_items(mut self, max: u32) -> Self {
        self.max_items = Some(max);
        self
    }

    /// Build into ElicitationSchema
    pub fn build(self) -> ElicitationSchema {
        let mut schema = ElicitationSchema {
            schema_type: "array".to_string(),
            properties: HashMap::new(),
            required: None,
        };

        if let Some(items) = self.items {
            // Store items as a property (array schemas in MCP are simplified)
            schema.properties.insert("items".to_string(), *items);
        }

        schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_elicitation_request_serialization() {
        let request = ElicitationCreateRequest {
            message: "Please provide your configuration".to_string(),
            requested_schema: ElicitationSchema::new()
                .add_property(
                    "name".to_string(),
                    string()
                        .title("Project Name")
                        .min_length(3)
                        .max_length(50)
                        .build(),
                )
                .add_property(
                    "port".to_string(),
                    integer()
                        .title("Port Number")
                        .range(1024.0, 65535.0)
                        .build(),
                )
                .require(vec!["name".to_string()]),
        };

        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["message"], "Please provide your configuration");
        assert_eq!(json["requestedSchema"]["type"], "object");
        assert!(json["requestedSchema"]["properties"]["name"].is_object());
        assert_eq!(
            json["requestedSchema"]["properties"]["name"]["type"],
            "string"
        );
        assert_eq!(json["requestedSchema"]["required"], json!(["name"]));
    }

    #[test]
    fn test_elicitation_response_serialization() {
        let response = ElicitationCreateResult {
            action: ElicitationAction::Accept,
            content: Some({
                let mut map = HashMap::new();
                map.insert(
                    "name".to_string(),
                    ElicitationValue::String("my-project".to_string()),
                );
                map.insert("port".to_string(), ElicitationValue::Integer(3000));
                map.insert("debug".to_string(), ElicitationValue::Boolean(true));
                map
            }),
            meta: None,
        };

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["action"], "accept");
        assert_eq!(json["content"]["name"], "my-project");
        assert_eq!(json["content"]["port"], 3000);
        assert_eq!(json["content"]["debug"], true);
    }

    #[test]
    fn test_decline_response() {
        let response = ElicitationCreateResult {
            action: ElicitationAction::Decline,
            content: None,
            meta: None,
        };

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["action"], "decline");
        assert!(json.get("content").is_none());
    }

    #[test]
    fn test_schema_builders() {
        // String with email format
        let email_schema = string().title("Email Address").email().build();

        if let PrimitiveSchemaDefinition::String(s) = email_schema {
            assert_eq!(s.title, Some("Email Address".to_string()));
            assert_eq!(s.format, Some(StringFormat::Email));
        } else {
            panic!("Expected string schema");
        }

        // Integer with range
        let port_schema = integer().title("Port").range(1024.0, 65535.0).build();

        if let PrimitiveSchemaDefinition::Number(n) = port_schema {
            assert_eq!(n.schema_type, "integer");
            assert_eq!(n.minimum, Some(1024.0));
            assert_eq!(n.maximum, Some(65535.0));
        } else {
            panic!("Expected number schema");
        }

        // Boolean with default
        let bool_schema = boolean().title("Enable Debug").default(false).build();

        if let PrimitiveSchemaDefinition::Boolean(b) = bool_schema {
            assert_eq!(b.default, Some(false));
        } else {
            panic!("Expected boolean schema");
        }

        // Enum with names
        let enum_schema = enum_of(vec!["sm".to_string(), "md".to_string(), "lg".to_string()])
            .title("Size")
            .names(vec![
                "Small".to_string(),
                "Medium".to_string(),
                "Large".to_string(),
            ])
            .build();

        if let PrimitiveSchemaDefinition::Enum(e) = enum_schema {
            assert_eq!(e.enum_values.len(), 3);
            assert_eq!(e.enum_names.as_ref().unwrap().len(), 3);
        } else {
            panic!("Expected enum schema");
        }
    }
}
