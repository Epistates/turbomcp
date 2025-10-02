//! Tool system types
//!
//! This module contains types for the MCP tool calling system, including
//! tool definitions, schemas, and tool execution requests/responses.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{content::ContentBlock, core::Cursor};

/// Tool-specific annotations for additional tool information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    /// Title for display purposes - takes precedence over name for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Audience-specific information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Priority for ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// If true, the tool may perform destructive updates to its environment
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    /// If true, calling the tool repeatedly with same arguments has no additional effect
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
    /// If true, this tool may interact with an "open world" of external entities
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: Option<bool>,
    /// If true, the tool does not modify its environment
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    /// Additional custom annotations
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Tool definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description of the tool
    /// This can be used by clients to improve the LLM's understanding of available tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema object defining the expected parameters for the tool
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,

    /// Optional JSON Schema object defining the structure of the tool's output
    /// returned in the structuredContent field of a CallToolResult
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ToolOutputSchema>,

    /// Optional additional tool information
    /// Display name precedence order is: title, annotations.title, then name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,

    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            name: "unnamed_tool".to_string(), // Must have a valid name for MCP compliance
            title: None,
            description: None,
            input_schema: ToolInputSchema::default(),
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }
}

impl Tool {
    /// Create a new tool with the given name
    ///
    /// # Panics
    /// Panics if the name is empty or contains only whitespace
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.trim().is_empty(), "Tool name cannot be empty");
        Self {
            name,
            title: None,
            description: None,
            input_schema: ToolInputSchema::default(),
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }

    /// Create a new tool with name and description
    ///
    /// # Panics
    /// Panics if the name is empty or contains only whitespace
    pub fn with_description(name: impl Into<String>, description: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.trim().is_empty(), "Tool name cannot be empty");
        Self {
            name,
            title: None,
            description: Some(description.into()),
            input_schema: ToolInputSchema::default(),
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }

    /// Set the input schema for this tool
    pub fn with_input_schema(mut self, schema: ToolInputSchema) -> Self {
        self.input_schema = schema;
        self
    }

    /// Set the output schema for this tool
    pub fn with_output_schema(mut self, schema: ToolOutputSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Set the title for this tool
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set annotations for this tool
    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

/// Tool input schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    /// Must be "object" for tool input schemas
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Schema properties defining the tool parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    /// List of required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional properties are allowed
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

impl Default for ToolInputSchema {
    fn default() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            additional_properties: None,
        }
    }
}

impl ToolInputSchema {
    /// Create a simple input schema with no properties
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a schema with properties (no required fields)
    pub fn with_properties(properties: HashMap<String, serde_json::Value>) -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: Some(properties),
            required: None,
            additional_properties: None,
        }
    }

    /// Create a schema with required properties
    pub fn with_required_properties(
        properties: HashMap<String, serde_json::Value>,
        required: Vec<String>,
    ) -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: Some(properties),
            required: Some(required),
            additional_properties: Some(false),
        }
    }

    /// Add a property to the schema
    pub fn add_property(mut self, name: String, property: serde_json::Value) -> Self {
        if self.properties.is_none() {
            self.properties = Some(HashMap::new());
        }
        if let Some(ref mut properties) = self.properties {
            properties.insert(name, property);
        }
        self
    }

    /// Mark a property as required
    pub fn require_property(mut self, name: String) -> Self {
        if self.required.is_none() {
            self.required = Some(Vec::new());
        }
        if let Some(ref mut required) = self.required
            && !required.contains(&name)
        {
            required.push(name);
        }
        self
    }
}

/// Tool output schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputSchema {
    /// Must be "object" for tool output schemas
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Schema properties defining the tool output structure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    /// List of required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional properties are allowed
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

/// List tools request with optional pagination
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListToolsRequest {
    /// Optional cursor for pagination
    /// An opaque token representing the current pagination position.
    /// If provided, the server should return results starting after this cursor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// List tools result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// Available tools
    pub tools: Vec<Tool>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Call tool request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    /// Tool name
    pub name: String,
    /// Tool arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, serde_json::Value>>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Call tool result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    /// Result content (required)
    pub content: Vec<ContentBlock>,
    /// Whether the operation failed
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Optional structured result of the tool call per MCP 2025-06-18 specification
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}
