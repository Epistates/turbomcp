//! Types for the MCP tool-calling system.
//!
//! This module defines the data structures for defining tools, their input/output schemas,
//! and the requests and responses used to list and execute them, as specified by the MCP standard.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{content::ContentBlock, core::Cursor};

/// Optional metadata hints about a tool's behavior.
///
/// **Critical Warning** (from MCP spec):
/// > "All properties in ToolAnnotations are **hints**. They are not guaranteed to
/// > provide a faithful description of tool behavior. **Clients should never make
/// > tool use decisions based on ToolAnnotations received from untrusted servers.**"
///
/// These fields are useful for UI display and general guidance, but should never
/// be trusted for security decisions or behavioral assumptions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    /// A user-friendly title for display in UIs (hint only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Role-based audience hint. Per spec, should be `"user"` or `"assistant"` (hint only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Subjective priority for UI sorting (hint only, often ignored).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// **Hint** that the tool may perform destructive actions (e.g., deleting data).
    ///
    /// Do not trust this for security decisions. Default: `true` if not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    /// **Hint** that repeated calls with same args have no additional effects.
    ///
    /// Useful for retry logic, but verify actual behavior. Default: `false` if not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
    /// **Hint** that the tool may interact with external systems or the real world.
    ///
    /// Do not trust this for sandboxing decisions. Default: `true` if not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: Option<bool>,
    /// **Hint** that the tool does not modify state (read-only).
    ///
    /// Do not trust this for security decisions. Default: `false` if not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    /// Custom application-specific hints.
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Represents a tool that can be executed by an MCP server, as per the MCP 2025-06-18 specification.
///
/// A `Tool` definition includes its programmatic name, a human-readable description,
/// and JSON schemas for its inputs and outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// The programmatic name of the tool, used to identify it in `CallToolRequest`.
    pub name: String,

    /// An optional, user-friendly title for the tool. Display name precedence is: `title`, `annotations.title`, then `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A human-readable description of what the tool does, which can be used by clients or LLMs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The JSON Schema object defining the parameters the tool accepts.
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,

    /// An optional JSON Schema object defining the structure of the tool's successful output.
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ToolOutputSchema>,

    /// Optional, additional metadata providing hints about the tool's behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,

    /// A general-purpose metadata field for custom data.
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
    /// Creates a new `Tool` with a given name.
    ///
    /// # Panics
    /// Panics if the name is empty or contains only whitespace.
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

    /// Creates a new `Tool` with a name and a description.
    ///
    /// # Panics
    /// Panics if the name is empty or contains only whitespace.
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

    /// Sets the input schema for this tool.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::types::{Tool, ToolInputSchema};
    /// let schema = ToolInputSchema::empty();
    /// let tool = Tool::new("my_tool").with_input_schema(schema);
    /// ```
    pub fn with_input_schema(mut self, schema: ToolInputSchema) -> Self {
        self.input_schema = schema;
        self
    }

    /// Sets the output schema for this tool.
    pub fn with_output_schema(mut self, schema: ToolOutputSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Sets the user-friendly title for this tool.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the annotations for this tool.
    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

/// Defines the structure of the arguments a tool accepts, as a JSON Schema object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    /// The type of the schema, which must be "object" for tool inputs.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// A map defining the properties (parameters) the tool accepts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    /// A list of property names that are required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional, unspecified properties are allowed.
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

impl Default for ToolInputSchema {
    /// Creates a default `ToolInputSchema` that accepts an empty object.
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
    /// Creates a new, empty input schema that accepts no parameters.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a new schema with a given set of properties.
    pub fn with_properties(properties: HashMap<String, serde_json::Value>) -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: Some(properties),
            required: None,
            additional_properties: None,
        }
    }

    /// Creates a new schema with a given set of properties and a list of required properties.
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

    /// Adds a property to the schema using a builder pattern.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::types::ToolInputSchema;
    /// # use serde_json::json;
    /// let schema = ToolInputSchema::empty()
    ///     .add_property("name".to_string(), json!({ "type": "string" }));
    /// ```
    pub fn add_property(mut self, name: String, property: serde_json::Value) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name, property);
        self
    }

    /// Marks a property as required using a builder pattern.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::types::ToolInputSchema;
    /// # use serde_json::json;
    /// let schema = ToolInputSchema::empty()
    ///     .add_property("name".to_string(), json!({ "type": "string" }))
    ///     .require_property("name".to_string());
    /// ```
    pub fn require_property(mut self, name: String) -> Self {
        let required = self.required.get_or_insert_with(Vec::new);
        if !required.contains(&name) {
            required.push(name);
        }
        self
    }
}

/// Defines the structure of a tool's successful output, as a JSON Schema object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputSchema {
    /// The type of the schema, which must be "object" for tool outputs.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// A map defining the properties of the output object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    /// A list of property names in the output that are required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional, unspecified properties are allowed in the output.
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

/// A request to list the available tools on a server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListToolsRequest {
    /// An optional cursor for pagination. If provided, the server should return
    /// the next page of results starting after this cursor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Optional metadata for the request.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// The result of a `ListToolsRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// The list of available tools for the current page.
    pub tools: Vec<Tool>,
    /// An optional continuation token for retrieving the next page of results.
    /// If `None`, there are no more results.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    /// Optional metadata for the result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// A request to execute a specific tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    /// The programmatic name of the tool to call.
    pub name: String,
    /// The arguments to pass to the tool, conforming to its `input_schema`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, serde_json::Value>>,
    /// Optional metadata for the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// The result of a `CallToolRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    /// The output of the tool, typically as a series of text or other content blocks. This is required.
    pub content: Vec<ContentBlock>,
    /// An optional boolean indicating whether the tool execution resulted in an error.
    ///
    /// When `is_error` is `true`, all content blocks should be treated as error information.
    /// The error message may span multiple text blocks for structured error reporting.
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Optional structured output from the tool, conforming to its `output_schema`.
    ///
    /// When present, this contains schema-validated JSON output that clients can parse
    /// and use programmatically. Tools that return structured content SHOULD also include
    /// the serialized JSON in a TextContent block for backward compatibility with clients
    /// that don't support structured output.
    ///
    /// See [`Tool::output_schema`] for defining the expected structure.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    /// Optional metadata for the result.
    ///
    /// This field is for client applications and tools to pass additional context that
    /// should NOT be exposed to LLMs. Examples include tracking IDs, performance metrics,
    /// cache status, or internal state information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl CallToolResult {
    /// Extracts and concatenates all text content from the result.
    ///
    /// This is useful for simple text-only tools or when you want to present
    /// all textual output as a single string.
    ///
    /// # Returns
    ///
    /// A single string containing all text blocks concatenated with newlines.
    /// Returns an empty string if there are no text blocks.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::types::{CallToolResult, ContentBlock, TextContent};
    ///
    /// let result = CallToolResult {
    ///     content: vec![
    ///         ContentBlock::Text(TextContent {
    ///             text: "Line 1".to_string(),
    ///             annotations: None,
    ///             meta: None,
    ///         }),
    ///         ContentBlock::Text(TextContent {
    ///             text: "Line 2".to_string(),
    ///             annotations: None,
    ///             meta: None,
    ///         }),
    ///     ],
    ///     is_error: None,
    ///     structured_content: None,
    ///     _meta: None,
    /// };
    ///
    /// assert_eq!(result.all_text(), "Line 1\nLine 2");
    /// ```
    pub fn all_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Returns the text content of the first text block, if any.
    ///
    /// This is a common pattern for simple tools that return a single text response.
    ///
    /// # Returns
    ///
    /// `Some(&str)` if the first content block is text, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::types::{CallToolResult, ContentBlock, TextContent};
    ///
    /// let result = CallToolResult {
    ///     content: vec![
    ///         ContentBlock::Text(TextContent {
    ///             text: "Hello, world!".to_string(),
    ///             annotations: None,
    ///             meta: None,
    ///         }),
    ///     ],
    ///     is_error: None,
    ///     structured_content: None,
    ///     _meta: None,
    /// };
    ///
    /// assert_eq!(result.first_text(), Some("Hello, world!"));
    /// ```
    pub fn first_text(&self) -> Option<&str> {
        self.content.first().and_then(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
    }

    /// Checks if the tool execution resulted in an error.
    ///
    /// # Returns
    ///
    /// `true` if `is_error` is explicitly set to `true`, `false` otherwise
    /// (including when `is_error` is `None`).
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::types::CallToolResult;
    ///
    /// let success_result = CallToolResult {
    ///     content: vec![],
    ///     is_error: Some(false),
    ///     structured_content: None,
    ///     _meta: None,
    /// };
    /// assert!(!success_result.has_error());
    ///
    /// let error_result = CallToolResult {
    ///     content: vec![],
    ///     is_error: Some(true),
    ///     structured_content: None,
    ///     _meta: None,
    /// };
    /// assert!(error_result.has_error());
    ///
    /// let unspecified_result = CallToolResult {
    ///     content: vec![],
    ///     is_error: None,
    ///     structured_content: None,
    ///     _meta: None,
    /// };
    /// assert!(!unspecified_result.has_error());
    /// ```
    pub fn has_error(&self) -> bool {
        self.is_error.unwrap_or(false)
    }

    /// Creates a user-friendly display string for the tool result.
    ///
    /// This method provides a formatted representation suitable for logging,
    /// debugging, or displaying to end users. It handles multiple content types
    /// and includes structured content and error information when present.
    ///
    /// # Returns
    ///
    /// A formatted string representing the tool result.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::types::{CallToolResult, ContentBlock, TextContent};
    ///
    /// let result = CallToolResult {
    ///     content: vec![
    ///         ContentBlock::Text(TextContent {
    ///             text: "Operation completed".to_string(),
    ///             annotations: None,
    ///             meta: None,
    ///         }),
    ///     ],
    ///     is_error: Some(false),
    ///     structured_content: None,
    ///     _meta: None,
    /// };
    ///
    /// let display = result.to_display_string();
    /// assert!(display.contains("Operation completed"));
    /// ```
    pub fn to_display_string(&self) -> String {
        let mut parts = Vec::new();

        // Add error indicator if present
        if self.has_error() {
            parts.push("ERROR:".to_string());
        }

        // Process content blocks
        for (i, block) in self.content.iter().enumerate() {
            match block {
                ContentBlock::Text(text) => {
                    parts.push(text.text.clone());
                }
                ContentBlock::Image(img) => {
                    parts.push(format!(
                        "[Image: {} bytes, type: {}]",
                        img.data.len(),
                        img.mime_type
                    ));
                }
                ContentBlock::Audio(audio) => {
                    parts.push(format!(
                        "[Audio: {} bytes, type: {}]",
                        audio.data.len(),
                        audio.mime_type
                    ));
                }
                ContentBlock::ResourceLink(link) => {
                    let desc = link.description.as_deref().unwrap_or("");
                    let mime = link
                        .mime_type
                        .as_deref()
                        .map(|m| format!(" [{}]", m))
                        .unwrap_or_default();
                    parts.push(format!(
                        "[Resource: {}{}{}{}]",
                        link.name,
                        mime,
                        if !desc.is_empty() { ": " } else { "" },
                        desc
                    ));
                }
                ContentBlock::Resource(_resource) => {
                    parts.push(format!("[Embedded Resource #{}]", i + 1));
                }
            }
        }

        // Add structured content indicator if present
        if self.structured_content.is_some() {
            parts.push("[Includes structured output]".to_string());
        }

        parts.join("\n")
    }
}
