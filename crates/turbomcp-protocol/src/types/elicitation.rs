//! User input elicitation types
//!
//! This module contains types for server-initiated user input requests:
//! - **Form Mode** (MCP 2025-11-25): In-band structured data collection
//! - **URL Mode** (MCP 2025-11-25 draft, SEP-1036): Out-of-band sensitive data collection
//!
//! ## Form Mode vs URL Mode
//!
//! | Aspect | Form Mode | URL Mode |
//! |--------|-----------|----------|
//! | **Data Flow** | In-band (through MCP) | Out-of-band (external URL) |
//! | **Use Case** | Non-sensitive structured data | Sensitive data, OAuth, credentials |
//! | **Security** | Data visible to MCP client | Data **NOT** visible to MCP client |

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Elicitation action taken by user
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ElicitationAction {
    /// User submitted the form/confirmed the action
    Accept,
    /// User explicitly declined the action
    Decline,
    /// User dismissed without making an explicit choice
    Cancel,
}

/// Schema for elicitation input validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationSchema {
    /// Schema type (must be "object", required by MCP spec)
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Schema properties (required by MCP spec)
    pub properties: HashMap<String, PrimitiveSchemaDefinition>,
    /// Required properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Additional properties allowed
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

impl ElicitationSchema {
    /// Create a new elicitation schema
    pub fn new() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: HashMap::new(),
            required: Some(Vec::new()),
            additional_properties: Some(false),
        }
    }

    /// Add a string property to the schema
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

        if required && let Some(ref mut required_fields) = self.required {
            required_fields.push(name);
        }

        self
    }

    /// Add a number property to the schema
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

        if required && let Some(ref mut required_fields) = self.required {
            required_fields.push(name);
        }

        self
    }

    /// Add a boolean property to the schema
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

        if required && let Some(ref mut required_fields) = self.required {
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

/// Primitive schema definition for elicitation fields
///
/// ## Version Support
/// - MCP 2025-11-25: String (with legacy enumNames), Number, Integer, Boolean
/// - MCP 2025-11-25 draft (SEP-1330): Use `EnumSchema` for standards-compliant enum handling
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PrimitiveSchemaDefinition {
    /// String field schema definition
    ///
    /// **Note**: For enum fields, prefer using `EnumSchema` (MCP 2025-11-25 draft)
    /// over the legacy `enum_values`/`enum_names` pattern for standards compliance.
    #[serde(rename = "string")]
    String {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// String format (email, uri, date, date-time, etc.)
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        /// Minimum string length
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "minLength")]
        min_length: Option<u32>,
        /// Maximum string length
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "maxLength")]
        max_length: Option<u32>,
        /// Allowed enum values
        ///
        /// **Legacy**: Use `EnumSchema::UntitledSingleSelect` instead (MCP 2025-11-25)
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "enum")]
        enum_values: Option<Vec<String>>,
        /// Display names for enum values
        ///
        /// **Deprecated**: This is non-standard JSON Schema. Use `EnumSchema::TitledSingleSelect`
        /// with `oneOf` pattern instead (MCP 2025-11-25 draft, SEP-1330).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "enumNames")]
        enum_names: Option<Vec<String>>,
    },
    /// Number field schema definition
    #[serde(rename = "number")]
    Number {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum value
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        /// Maximum value
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    /// Integer field schema definition
    #[serde(rename = "integer")]
    Integer {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum value
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<i64>,
        /// Maximum value
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<i64>,
    },
    /// Boolean field schema definition
    #[serde(rename = "boolean")]
    Boolean {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Default value
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
}

// ========== MCP 2025-11-25 Draft: Standards-Based Enum Schemas (SEP-1330) ==========

/// Enum option with value and display title (JSON Schema 2020-12 compliant)
///
/// Used with `oneOf` (single-select) or `anyOf` (multi-select) patterns
/// to provide human-readable labels for enum values.
///
/// ## Example
/// ```json
/// { "const": "#FF0000", "title": "Red" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumOption {
    /// The enum value (must match one of the allowed values)
    #[serde(rename = "const")]
    pub const_value: String,
    /// Human-readable display name for this option
    pub title: String,
}

/// Single-select enum schema with display titles (JSON Schema 2020-12 compliant)
///
/// Replaces the legacy `enum + enumNames` pattern with standards-compliant `oneOf` + `const`.
///
/// ## Example
/// ```json
/// {
///   "type": "string",
///   "title": "Color Selection",
///   "oneOf": [
///     { "const": "#FF0000", "title": "Red" },
///     { "const": "#00FF00", "title": "Green" }
///   ],
///   "default": "#FF0000"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitledSingleSelectEnumSchema {
    /// Schema type (must be "string")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Array of enum options with const/title pairs (JSON Schema 2020-12)
    #[serde(rename = "oneOf")]
    pub one_of: Vec<EnumOption>,
    /// Optional schema title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional schema description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value (must match one of the const values)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Single-select enum schema without display titles (standard JSON Schema)
///
/// Simple enum pattern without custom titles - uses enum values as labels.
///
/// ## Example
/// ```json
/// {
///   "type": "string",
///   "enum": ["red", "green", "blue"],
///   "default": "red"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UntitledSingleSelectEnumSchema {
    /// Schema type (must be "string")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Array of allowed string values (standard JSON Schema)
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
    /// Optional schema title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional schema description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value (must match one of the enum values)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Multi-select enum schema with display titles (JSON Schema 2020-12 compliant)
///
/// Allows selecting multiple values from an enumeration with human-readable labels.
/// Uses `anyOf` pattern for array items.
///
/// ## Example
/// ```json
/// {
///   "type": "array",
///   "title": "Color Selection",
///   "minItems": 1,
///   "maxItems": 2,
///   "items": {
///     "anyOf": [
///       { "const": "#FF0000", "title": "Red" },
///       { "const": "#00FF00", "title": "Green" }
///     ]
///   },
///   "default": ["#FF0000"]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitledMultiSelectEnumSchema {
    /// Schema type (must be "array")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Minimum number of selections required
    #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Maximum number of selections allowed
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    /// Array item schema with anyOf enum options
    pub items: MultiSelectItems,
    /// Optional schema title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional schema description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value (array of selected values)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
}

/// Multi-select enum schema without display titles (standard JSON Schema)
///
/// Simple multi-select pattern using enum array.
///
/// ## Example
/// ```json
/// {
///   "type": "array",
///   "items": {
///     "type": "string",
///     "enum": ["red", "green", "blue"]
///   },
///   "default": ["red", "green"]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UntitledMultiSelectEnumSchema {
    /// Schema type (must be "array")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Minimum number of selections required
    #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Maximum number of selections allowed
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    /// Array item schema with simple enum
    pub items: UntitledMultiSelectItems,
    /// Optional schema title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional schema description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value (array of selected values)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
}

/// Array items schema for titled multi-select (using anyOf)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSelectItems {
    /// Array of enum options with const/title pairs (JSON Schema 2020-12)
    #[serde(rename = "anyOf")]
    pub any_of: Vec<EnumOption>,
}

/// Array items schema for untitled multi-select (using simple enum)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UntitledMultiSelectItems {
    /// Item type (must be "string")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Array of allowed string values
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
}

/// Legacy enum schema with enumNames (deprecated, non-standard)
///
/// **Deprecated**: This uses the non-standard `enumNames` keyword which is not part of
/// JSON Schema 2020-12. Use `TitledSingleSelectEnumSchema` with `oneOf` pattern instead.
///
/// This type is maintained for backward compatibility only and will be removed
/// in a future version of the MCP specification.
///
/// ## Example
/// ```json
/// {
///   "type": "string",
///   "enum": ["#FF0000", "#00FF00"],
///   "enumNames": ["Red", "Green"]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyTitledEnumSchema {
    /// Schema type (must be "string")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Array of allowed values
    #[serde(rename = "enum")]
    pub enum_values: Vec<String>,
    /// Display names for enum values (non-standard, deprecated)
    #[serde(rename = "enumNames")]
    pub enum_names: Vec<String>,
    /// Optional schema title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional schema description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Standards-based enum schema (MCP 2025-11-25 draft, SEP-1330)
///
/// Replaces non-standard `enumNames` pattern with JSON Schema 2020-12 compliant
/// `oneOf`/`const` (single-select) and `anyOf` (multi-select) patterns.
///
/// ## Variants
///
/// - **TitledSingleSelect**: Single-select with display titles (oneOf + const)
/// - **UntitledSingleSelect**: Single-select without titles (simple enum)
/// - **TitledMultiSelect**: Multi-select with display titles (array + anyOf)
/// - **UntitledMultiSelect**: Multi-select without titles (array + enum)
/// - **LegacyTitled**: Backward compatibility (enum + enumNames, deprecated)
///
/// ## Standards Compliance
///
/// The new patterns use only standard JSON Schema 2020-12 keywords:
/// - `oneOf` with `const` and `title` for discriminated unions
/// - `anyOf` for array items with multiple allowed types
/// - `enum` for simple value restrictions
///
/// The legacy `enumNames` keyword was never part of any JSON Schema specification
/// and has been replaced with standards-compliant patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnumSchema {
    /// Single-select enum with display titles (oneOf pattern)
    TitledSingleSelect(TitledSingleSelectEnumSchema),
    /// Single-select enum without titles (simple enum)
    UntitledSingleSelect(UntitledSingleSelectEnumSchema),
    /// Multi-select enum with display titles (array + anyOf)
    TitledMultiSelect(TitledMultiSelectEnumSchema),
    /// Multi-select enum without titles (array + enum)
    UntitledMultiSelect(UntitledMultiSelectEnumSchema),
    /// Legacy enum with enumNames (deprecated, for backward compatibility)
    LegacyTitled(LegacyTitledEnumSchema),
}

/// Elicit request mode (MCP 2025-11-25 draft, SEP-1036)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ElicitMode {
    /// Form mode: In-band structured data collection (MCP 2025-11-25)
    Form,
    /// URL mode: Out-of-band sensitive data collection (MCP 2025-11-25 draft)
    Url,
}

/// URL mode elicitation parameters (MCP 2025-11-25 draft, SEP-1036)
///
/// Used for out-of-band interactions where sensitive information must not
/// pass through the MCP client (e.g., OAuth flows, API keys, credentials).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct URLElicitRequestParams {
    /// Elicitation mode (must be "url")
    pub mode: ElicitMode,

    /// Unique identifier for this elicitation
    #[serde(rename = "elicitationId")]
    pub elicitation_id: String,

    /// Human-readable message explaining why the interaction is needed
    pub message: String,

    /// URL user should navigate to (MUST NOT contain sensitive information)
    #[serde(with = "url_serde")]
    pub url: url::Url,
}

// Custom serde for url::Url to serialize as string
mod url_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use url::Url;

    pub(super) fn serialize<S>(url: &Url, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(url.as_str())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Url, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Url::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Form mode elicitation parameters (MCP 2025-11-25)
///
/// Used for in-band structured data collection with JSON schema validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormElicitRequestParams {
    /// Human-readable message for the user
    pub message: String,

    /// Schema for input validation (per MCP specification)
    #[serde(rename = "requestedSchema")]
    pub schema: ElicitationSchema,

    /// Optional timeout in milliseconds
    #[serde(rename = "timeoutMs", skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,

    /// Whether the request can be cancelled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancellable: Option<bool>,
}

/// Elicit request parameters (supports both form and URL modes)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ElicitRequestParams {
    /// Form mode: In-band structured data collection
    Form(FormElicitRequestParams),

    /// URL mode: Out-of-band sensitive data collection (MCP 2025-11-25 draft)
    Url(URLElicitRequestParams),
}

impl ElicitRequestParams {
    /// Create a new form mode elicitation request
    pub fn form(
        message: String,
        schema: ElicitationSchema,
        timeout_ms: Option<u32>,
        cancellable: Option<bool>,
    ) -> Self {
        ElicitRequestParams::Form(FormElicitRequestParams {
            message,
            schema,
            timeout_ms,
            cancellable,
        })
    }

    /// Create a new URL mode elicitation request
    pub fn url(elicitation_id: String, message: String, url: url::Url) -> Self {
        ElicitRequestParams::Url(URLElicitRequestParams {
            mode: ElicitMode::Url,
            elicitation_id,
            message,
            url,
        })
    }

    /// Get the message for this elicitation
    pub fn message(&self) -> &str {
        match self {
            ElicitRequestParams::Form(form) => &form.message,
            ElicitRequestParams::Url(url_params) => &url_params.message,
        }
    }
}

/// Elicit request wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitRequest {
    /// Elicitation parameters
    #[serde(flatten)]
    pub params: ElicitRequestParams,
    /// Task metadata for task-augmented elicitation (MCP 2025-11-25 draft, SEP-1686)
    ///
    /// When present, indicates the server should execute this elicitation request as a long-running
    /// task and return a CreateTaskResult instead of the immediate ElicitResult.
    /// The actual result can be retrieved later via tasks/result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<crate::types::tasks::TaskMetadata>,
    /// Optional metadata per MCP 2025-11-25 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl Default for ElicitRequest {
    fn default() -> Self {
        Self {
            params: ElicitRequestParams::form(String::new(), ElicitationSchema::new(), None, None),
            task: None,
            _meta: None,
        }
    }
}

/// Elicit result
///
/// ## Version Support
/// - MCP 2025-11-25: action, content (form mode), _meta
/// - MCP 2025-11-25 draft (SEP-1330): Clarified content field behavior
///
/// ## Content Field Behavior (SEP-1330 Clarification)
///
/// The `content` field is **only present** when:
/// 1. `action` is `"accept"` (user submitted the form), AND
/// 2. Mode was `"form"` (in-band structured data collection)
///
/// The `content` field is **omitted** when:
/// - Action is `"decline"` or `"cancel"`
/// - Mode was `"url"` (out-of-band, data doesn't transit through MCP)
///
/// ## Example
///
/// **Form mode with accept:**
/// ```json
/// {
///   "action": "accept",
///   "content": {
///     "name": "Alice",
///     "email": "alice@example.com"
///   }
/// }
/// ```
///
/// **URL mode with accept:**
/// ```json
/// {
///   "action": "accept"
/// }
/// ```
/// Note: No `content` field - data was collected out-of-band
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitResult {
    /// The user action in response to the elicitation
    ///
    /// - `accept`: User submitted the form/confirmed the action
    /// - `decline`: User explicitly declined the action
    /// - `cancel`: User dismissed without making an explicit choice
    pub action: ElicitationAction,

    /// The submitted form data, only present when action is "accept"
    /// and mode was "form".
    ///
    /// Contains values matching the requested schema. Omitted for:
    /// - `action` is `"decline"` or `"cancel"`
    /// - URL mode responses (out-of-band data collection)
    ///
    /// Per MCP 2025-11-25 draft (SEP-1330), this clarification ensures
    /// clients understand when to expect form data vs. out-of-band completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<std::collections::HashMap<String, serde_json::Value>>,

    /// Optional metadata per MCP 2025-11-25 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Elicitation completion notification parameters (MCP 2025-11-25 draft, SEP-1036)
///
/// Sent by the server to indicate that an out-of-band elicitation has been completed.
/// This allows the client to know when to retry the original request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationCompleteParams {
    /// The ID of the completed elicitation
    #[serde(rename = "elicitationId")]
    pub elicitation_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elicitation_action_serialization() {
        assert_eq!(
            serde_json::to_string(&ElicitationAction::Accept).unwrap(),
            "\"accept\""
        );
        assert_eq!(
            serde_json::to_string(&ElicitationAction::Decline).unwrap(),
            "\"decline\""
        );
        assert_eq!(
            serde_json::to_string(&ElicitationAction::Cancel).unwrap(),
            "\"cancel\""
        );
    }

    #[test]
    fn test_form_elicit_params() {
        let schema = ElicitationSchema::new().add_string_property(
            "name".to_string(),
            true,
            Some("User name".to_string()),
        );

        let params = ElicitRequestParams::form(
            "Please provide your name".to_string(),
            schema,
            Some(30000),
            Some(true),
        );

        assert_eq!(params.message(), "Please provide your name");

        // Test serialization
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("Please provide your name"));
        assert!(json.contains("requestedSchema"));
    }

    #[test]
    fn test_url_elicit_params() {
        use url::Url;

        let url = Url::parse("https://example.com/oauth/authorize").unwrap();
        let params = ElicitRequestParams::url(
            "test-id-123".to_string(),
            "Please authorize the connection".to_string(),
            url,
        );

        assert_eq!(params.message(), "Please authorize the connection");

        // Test serialization
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("test-id-123"));
        assert!(json.contains("https://example.com/oauth/authorize"));
        assert!(json.contains("\"mode\":\"url\""));
    }

    #[test]
    fn test_elicit_mode_serialization() {
        assert_eq!(
            serde_json::to_string(&ElicitMode::Form).unwrap(),
            "\"form\""
        );
        assert_eq!(serde_json::to_string(&ElicitMode::Url).unwrap(), "\"url\"");
    }

    #[test]
    fn test_completion_notification() {
        let params = ElicitationCompleteParams {
            elicitation_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        };

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("550e8400-e29b-41d4-a716-446655440000"));
        assert!(json.contains("elicitationId"));
    }

    #[test]
    fn test_elicit_result_form_mode() {
        let mut content = std::collections::HashMap::new();
        content.insert("name".to_string(), serde_json::json!("Alice"));

        let result = ElicitResult {
            action: ElicitationAction::Accept,
            content: Some(content),
            _meta: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"action\":\"accept\""));
        assert!(json.contains("\"name\":\"Alice\""));
    }

    #[test]
    fn test_elicit_result_url_mode() {
        // URL mode: no content field
        let result = ElicitResult {
            action: ElicitationAction::Accept,
            content: None,
            _meta: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"action\":\"accept\""));
        assert!(!json.contains("content"));
    }

    // ========== SEP-1330 Enum Schema Tests ==========

    #[test]
    fn test_titled_single_select_enum_schema() {
        use super::{EnumOption, TitledSingleSelectEnumSchema};

        let schema = TitledSingleSelectEnumSchema {
            schema_type: "string".to_string(),
            one_of: vec![
                EnumOption {
                    const_value: "#FF0000".to_string(),
                    title: "Red".to_string(),
                },
                EnumOption {
                    const_value: "#00FF00".to_string(),
                    title: "Green".to_string(),
                },
                EnumOption {
                    const_value: "#0000FF".to_string(),
                    title: "Blue".to_string(),
                },
            ],
            title: Some("Color Selection".to_string()),
            description: Some("Choose your favorite color".to_string()),
            default: Some("#FF0000".to_string()),
        };

        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"string\""));
        assert!(json.contains("\"oneOf\""));
        assert!(json.contains("\"const\":\"#FF0000\""));
        assert!(json.contains("\"title\":\"Red\""));
        assert!(json.contains("\"default\":\"#FF0000\""));

        // Verify deserialization
        let deserialized: TitledSingleSelectEnumSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.one_of.len(), 3);
        assert_eq!(deserialized.one_of[0].const_value, "#FF0000");
        assert_eq!(deserialized.one_of[0].title, "Red");
    }

    #[test]
    fn test_untitled_single_select_enum_schema() {
        use super::UntitledSingleSelectEnumSchema;

        let schema = UntitledSingleSelectEnumSchema {
            schema_type: "string".to_string(),
            enum_values: vec!["red".to_string(), "green".to_string(), "blue".to_string()],
            title: Some("Color Selection".to_string()),
            description: Some("Choose a color".to_string()),
            default: Some("red".to_string()),
        };

        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"string\""));
        assert!(json.contains("\"enum\""));
        assert!(json.contains("\"red\""));
        assert!(json.contains("\"green\""));
        assert!(json.contains("\"blue\""));
        assert!(!json.contains("oneOf"));

        // Verify deserialization
        let deserialized: UntitledSingleSelectEnumSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.enum_values.len(), 3);
        assert_eq!(deserialized.enum_values[0], "red");
    }

    #[test]
    fn test_titled_multi_select_enum_schema() {
        use super::{EnumOption, MultiSelectItems, TitledMultiSelectEnumSchema};

        let schema = TitledMultiSelectEnumSchema {
            schema_type: "array".to_string(),
            min_items: Some(1),
            max_items: Some(2),
            items: MultiSelectItems {
                any_of: vec![
                    EnumOption {
                        const_value: "#FF0000".to_string(),
                        title: "Red".to_string(),
                    },
                    EnumOption {
                        const_value: "#00FF00".to_string(),
                        title: "Green".to_string(),
                    },
                ],
            },
            title: Some("Color Selection".to_string()),
            description: Some("Choose up to 2 colors".to_string()),
            default: Some(vec!["#FF0000".to_string()]),
        };

        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"array\""));
        assert!(json.contains("\"minItems\":1"));
        assert!(json.contains("\"maxItems\":2"));
        assert!(json.contains("\"anyOf\""));
        assert!(json.contains("\"const\":\"#FF0000\""));

        // Verify deserialization
        let deserialized: TitledMultiSelectEnumSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.items.any_of.len(), 2);
        assert_eq!(deserialized.min_items, Some(1));
        assert_eq!(deserialized.max_items, Some(2));
    }

    #[test]
    fn test_untitled_multi_select_enum_schema() {
        use super::{UntitledMultiSelectEnumSchema, UntitledMultiSelectItems};

        let schema = UntitledMultiSelectEnumSchema {
            schema_type: "array".to_string(),
            min_items: Some(1),
            max_items: None,
            items: UntitledMultiSelectItems {
                schema_type: "string".to_string(),
                enum_values: vec!["red".to_string(), "green".to_string(), "blue".to_string()],
            },
            title: Some("Color Selection".to_string()),
            description: Some("Choose colors".to_string()),
            default: Some(vec!["red".to_string(), "green".to_string()]),
        };

        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"array\""));
        assert!(json.contains("\"minItems\":1"));
        assert!(json.contains("\"items\""));
        assert!(json.contains("\"enum\""));
        assert!(!json.contains("anyOf"));

        // Verify deserialization
        let deserialized: UntitledMultiSelectEnumSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.items.enum_values.len(), 3);
        assert_eq!(deserialized.default.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_legacy_titled_enum_schema() {
        use super::LegacyTitledEnumSchema;

        let schema = LegacyTitledEnumSchema {
            schema_type: "string".to_string(),
            enum_values: vec!["#FF0000".to_string(), "#00FF00".to_string()],
            enum_names: vec!["Red".to_string(), "Green".to_string()],
            title: Some("Color Selection".to_string()),
            description: Some("Choose a color (legacy)".to_string()),
            default: Some("#FF0000".to_string()),
        };

        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"string\""));
        assert!(json.contains("\"enum\""));
        assert!(json.contains("\"enumNames\""));
        assert!(json.contains("\"Red\""));
        assert!(!json.contains("oneOf"));

        // Verify deserialization
        let deserialized: LegacyTitledEnumSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.enum_values.len(), 2);
        assert_eq!(deserialized.enum_names.len(), 2);
        assert_eq!(deserialized.enum_names[0], "Red");
    }

    #[test]
    fn test_enum_schema_union_type() {
        use super::EnumSchema;

        // Test that EnumSchema can deserialize different variants
        let titled_json = r#"{
            "type": "string",
            "oneOf": [
                {"const": "red", "title": "Red"},
                {"const": "green", "title": "Green"}
            ]
        }"#;

        let schema: EnumSchema = serde_json::from_str(titled_json).unwrap();
        match schema {
            EnumSchema::TitledSingleSelect(s) => {
                assert_eq!(s.one_of.len(), 2);
                assert_eq!(s.one_of[0].const_value, "red");
            }
            _ => panic!("Expected TitledSingleSelect variant"),
        }

        // Test untitled variant
        let untitled_json = r#"{
            "type": "string",
            "enum": ["red", "green", "blue"]
        }"#;

        let schema: EnumSchema = serde_json::from_str(untitled_json).unwrap();
        match schema {
            EnumSchema::UntitledSingleSelect(s) => {
                assert_eq!(s.enum_values.len(), 3);
            }
            _ => panic!("Expected UntitledSingleSelect variant"),
        }
    }

    #[test]
    fn test_enum_option_serialization() {
        use super::EnumOption;

        let option = EnumOption {
            const_value: "#FF0000".to_string(),
            title: "Red".to_string(),
        };

        let json = serde_json::to_string(&option).unwrap();
        assert!(json.contains("\"const\":\"#FF0000\""));
        assert!(json.contains("\"title\":\"Red\""));
        assert!(!json.contains("const_value")); // Verifies camelCase rename

        // Verify deserialization
        let deserialized: EnumOption = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.const_value, "#FF0000");
        assert_eq!(deserialized.title, "Red");
    }
}
