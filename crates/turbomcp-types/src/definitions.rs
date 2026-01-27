//! Definition types for MCP capabilities.
//!
//! This module defines the metadata types that describe MCP server capabilities:
//! - `Tool` - Tool definitions with input schemas
//! - `Resource` - Resource definitions with URI templates
//! - `Prompt` - Prompt definitions with arguments
//! - `ServerInfo` - Server identification and version

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Server information for MCP initialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    /// Server name (machine-readable identifier)
    pub name: String,
    /// Server version
    pub version: String,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Server description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
}

impl ServerInfo {
    /// Create server info with name and version.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    /// Set the title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the icon.
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Icon for tools, resources, prompts, or servers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Icon {
    /// Data URI (embedded icon)
    DataUri(String),
    /// HTTP URL to icon
    Url(String),
}

/// Tool definition.
///
/// Describes a callable tool with its input schema and metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Tool {
    /// Tool name (machine-readable identifier)
    pub name: String,
    /// Tool description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for input parameters
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Tool icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Tool annotations (hints about behavior)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    /// Output schema for structured results
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// Extension metadata (tags, version, etc.)
    ///
    /// This field can contain arbitrary key-value pairs for extensibility.
    /// Common keys:
    /// - `tags`: Array of strings for categorization
    /// - `version`: Semantic version string
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<std::collections::HashMap<String, Value>>,
}

impl Tool {
    /// Create a new tool with name and description.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            input_schema: ToolInputSchema::default(),
            ..Default::default()
        }
    }

    /// Set the input schema.
    #[must_use]
    pub fn with_schema(mut self, schema: ToolInputSchema) -> Self {
        self.input_schema = schema;
        self
    }

    /// Set the output schema.
    #[must_use]
    pub fn with_output_schema(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Set the annotations.
    #[must_use]
    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Mark as read-only (hint for clients).
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.annotations = Some(self.annotations.unwrap_or_default().with_read_only(true));
        self
    }

    /// Mark as destructive (hint for clients).
    #[must_use]
    pub fn destructive(mut self) -> Self {
        self.annotations = Some(self.annotations.unwrap_or_default().with_destructive(true));
        self
    }
}

/// JSON Schema for tool input parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolInputSchema {
    /// Schema type (always "object" for MCP tools)
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Property definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Value>,
    /// Required property names
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
            schema_type: "object".into(),
            properties: None,
            required: None,
            additional_properties: Some(false),
        }
    }
}

impl ToolInputSchema {
    /// Create an empty object schema.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create from a JSON value (typically from schemars).
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        serde_json::from_value(value).unwrap_or_default()
    }
}

/// Annotations for tools describing behavior hints.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolAnnotations {
    /// Hint that this tool is read-only
    #[serde(rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Hint that this tool has destructive effects
    #[serde(rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Hint that this tool is idempotent
    #[serde(rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// Hint that this tool operates on an open world
    #[serde(rename = "openWorldHint", skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl ToolAnnotations {
    /// Set the read-only hint.
    #[must_use]
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only_hint = Some(value);
        self
    }

    /// Set the destructive hint.
    #[must_use]
    pub fn with_destructive(mut self, value: bool) -> Self {
        self.destructive_hint = Some(value);
        self
    }

    /// Set the idempotent hint.
    #[must_use]
    pub fn with_idempotent(mut self, value: bool) -> Self {
        self.idempotent_hint = Some(value);
        self
    }

    /// Set the open world hint.
    #[must_use]
    pub fn with_open_world(mut self, value: bool) -> Self {
        self.open_world_hint = Some(value);
        self
    }
}

/// Resource definition.
///
/// Describes a readable resource with its URI template and metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    /// Resource URI or URI template
    pub uri: String,
    /// Resource name (machine-readable identifier)
    pub name: String,
    /// Resource description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Resource icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// MIME type of the resource content
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Resource annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ResourceAnnotations>,
    /// Size in bytes (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Extension metadata (tags, version, etc.)
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<std::collections::HashMap<String, Value>>,
}

impl Resource {
    /// Create a new resource with URI and name.
    #[must_use]
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the MIME type.
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Set the size.
    #[must_use]
    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }
}

/// Annotations for resources.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourceAnnotations {
    /// Target audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<crate::Role>>,
    /// Priority level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
}

/// Resource template definition.
///
/// Describes a URI template for dynamic resources.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourceTemplate {
    /// URI template (RFC 6570)
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    /// Template name
    pub name: String,
    /// Template description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Template icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// MIME type of resources from this template
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Template annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ResourceAnnotations>,
}

impl ResourceTemplate {
    /// Create a new resource template.
    #[must_use]
    pub fn new(uri_template: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri_template: uri_template.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Prompt definition.
///
/// Describes a retrievable prompt with its arguments and metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Prompt {
    /// Prompt name (machine-readable identifier)
    pub name: String,
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Prompt icon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Prompt arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
    /// Extension metadata (tags, version, etc.)
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<std::collections::HashMap<String, Value>>,
}

impl Prompt {
    /// Create a new prompt with name and description.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            ..Default::default()
        }
    }

    /// Add an argument to the prompt.
    #[must_use]
    pub fn with_argument(mut self, arg: PromptArgument) -> Self {
        self.arguments.get_or_insert_with(Vec::new).push(arg);
        self
    }

    /// Add a required argument.
    #[must_use]
    pub fn with_required_arg(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.with_argument(PromptArgument::required(name, description))
    }

    /// Add an optional argument.
    #[must_use]
    pub fn with_optional_arg(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.with_argument(PromptArgument::optional(name, description))
    }
}

/// Argument definition for prompts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptArgument {
    /// Argument name
    pub name: String,
    /// Argument description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this argument is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl PromptArgument {
    /// Create a required argument.
    #[must_use]
    pub fn required(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            required: Some(true),
        }
    }

    /// Create an optional argument.
    #[must_use]
    pub fn optional(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            required: Some(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_info() {
        let info = ServerInfo::new("my-server", "1.0.0")
            .with_title("My Server")
            .with_description("A test server");

        assert_eq!(info.name, "my-server");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.title, Some("My Server".into()));
    }

    #[test]
    fn test_tool_builder() {
        // Test with_annotations directly
        let tool = Tool::new("add", "Add two numbers").with_annotations(
            ToolAnnotations::default()
                .with_read_only(true)
                .with_idempotent(true),
        );

        assert_eq!(tool.name, "add");
        assert!(tool.annotations.as_ref().unwrap().read_only_hint.unwrap());
        assert!(tool.annotations.as_ref().unwrap().idempotent_hint.unwrap());
    }

    #[test]
    fn test_tool_read_only() {
        let tool = Tool::new("query", "Query data").read_only();
        assert!(tool.annotations.as_ref().unwrap().read_only_hint.unwrap());
    }

    #[test]
    fn test_tool_destructive() {
        let tool = Tool::new("delete", "Delete data").destructive();
        assert!(tool.annotations.as_ref().unwrap().destructive_hint.unwrap());
    }

    #[test]
    fn test_resource_builder() {
        let resource = Resource::new("file:///test.txt", "test")
            .with_description("A test file")
            .with_mime_type("text/plain");

        assert_eq!(resource.uri, "file:///test.txt");
        assert_eq!(resource.mime_type, Some("text/plain".into()));
    }

    #[test]
    fn test_prompt_builder() {
        let prompt = Prompt::new("greeting", "A greeting prompt")
            .with_required_arg("name", "Name to greet")
            .with_optional_arg("style", "Greeting style");

        assert_eq!(prompt.name, "greeting");
        assert_eq!(prompt.arguments.as_ref().unwrap().len(), 2);
        assert!(prompt.arguments.as_ref().unwrap()[0].required.unwrap());
        assert!(!prompt.arguments.as_ref().unwrap()[1].required.unwrap());
    }

    #[test]
    fn test_tool_serde() {
        let tool = Tool::new("test", "Test tool");
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"name\":\"test\""));
        assert!(json.contains("\"inputSchema\""));
    }
}
