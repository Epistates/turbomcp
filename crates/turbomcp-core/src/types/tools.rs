//! Tool types for MCP.

use alloc::string::String;
use alloc::vec::Vec;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::content::Content;
use super::core::Icon;

/// Tool definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name (programmatic identifier)
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for input parameters
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,
    /// Optional display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional icon (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Tool annotations (hints)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

impl Tool {
    /// Create a new tool
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the description
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the input schema
    #[must_use]
    pub fn with_input_schema(mut self, schema: ToolInputSchema) -> Self {
        self.input_schema = schema;
        self
    }

    /// Set the icon (MCP 2025-11-25)
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// JSON Schema for tool input
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolInputSchema {
    /// Schema type (always "object")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Property definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
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

impl ToolInputSchema {
    /// Create an empty object schema
    #[must_use]
    pub fn object() -> Self {
        Self {
            schema_type: "object".into(),
            properties: Some(HashMap::new()),
            required: None,
            additional_properties: Some(false),
        }
    }

    /// Add a property
    #[must_use]
    pub fn with_property(mut self, name: impl Into<String>, schema: Value, required: bool) -> Self {
        let name = name.into();
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.clone(), schema);
        if required {
            self.required.get_or_insert_with(Vec::new).push(name);
        }
        self
    }
}

/// Tool-specific annotations (hints)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolAnnotations {
    /// Hint that tool performs destructive operations
    #[serde(rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Hint that tool is read-only
    #[serde(rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Hint that tool is idempotent
    #[serde(rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// Hint for user interaction
    #[serde(rename = "openWorldHint", skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    /// Display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Request to list available tools
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListToolsRequest {
    /// Pagination cursor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Response with list of tools
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// Available tools
    pub tools: Vec<Tool>,
    /// Next page cursor
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Request to call a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    /// Tool name to call
    pub name: String,
    /// Tool arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, Value>>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Result of a tool call
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CallToolResult {
    /// Tool output content
    pub content: Vec<Content>,
    /// Whether the tool call resulted in an error
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

impl CallToolResult {
    /// Create a successful result with text content
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: alloc::vec![Content::text(text)],
            is_error: None,
            _meta: None,
        }
    }

    /// Create a successful JSON result
    ///
    /// Serializes the value to pretty-printed JSON text.
    pub fn json<T: serde::Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self::text(text))
    }

    /// Create an error result
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: alloc::vec![Content::text(message)],
            is_error: Some(true),
            _meta: None,
        }
    }

    /// Create a result with multiple content items
    #[must_use]
    pub fn contents(contents: Vec<Content>) -> Self {
        Self {
            content: contents,
            is_error: None,
            _meta: None,
        }
    }

    /// Create an image result (base64 encoded)
    #[must_use]
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            content: alloc::vec![Content::image(data, mime_type)],
            is_error: None,
            _meta: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_builder() {
        let tool = Tool::new("calculator")
            .with_description("Performs calculations")
            .with_input_schema(ToolInputSchema::object());

        assert_eq!(tool.name, "calculator");
        assert!(tool.description.is_some());
    }

    #[test]
    fn test_call_tool_result() {
        let result = CallToolResult::text("Hello");
        assert_eq!(result.content.len(), 1);
        assert!(result.is_error.is_none());

        let error = CallToolResult::error("Failed");
        assert_eq!(error.is_error, Some(true));
    }
}
