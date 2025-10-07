//! User input elicitation types (MCP 2025-06-18)
//!
//! This module contains types for server-initiated user input requests,
//! allowing servers to request structured input from users.

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PrimitiveSchemaDefinition {
    /// String field schema definition
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
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "enum")]
        enum_values: Option<Vec<String>>,
        /// Display names for enum values
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

/// Elicit request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitRequestParams {
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

/// Elicit request wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitRequest {
    /// Elicitation parameters
    #[serde(flatten)]
    pub params: ElicitRequestParams,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Elicit result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitResult {
    /// The action taken by the user
    pub action: ElicitationAction,
    /// User input content (if action was Accept) - per MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}
