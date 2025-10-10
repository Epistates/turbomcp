//! Types for the MCP tool-calling system.
//!
//! This module defines the data structures for defining tools, their input/output schemas,
//! and the requests and responses used to list and execute them, as specified by the MCP standard.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{content::ContentBlock, core::Cursor};

/// Provides additional, optional metadata about a tool.
///
/// These annotations offer hints to clients and LLMs about the tool's behavior,
/// helping them make more informed decisions about when and how to use the tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    /// A user-friendly title for the tool, which may be used in UIs instead of the programmatic `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Specifies the intended audience for the tool (e.g., "developer", "admin").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// A numeric value indicating the tool's priority, useful for sorting or ranking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// If `true`, hints that the tool may perform destructive actions (e.g., deleting data).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    /// If `true`, hints that calling the tool multiple times with the same arguments will not have additional effects.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
    /// If `true`, hints that the tool may interact with external systems or the real world.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: Option<bool>,
    /// If `true`, hints that the tool does not modify any state and only reads data.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    /// A map for any other custom annotations not defined in the specification.
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
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Optional structured output from the tool, conforming to its `output_schema`.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    /// Optional metadata for the result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}
