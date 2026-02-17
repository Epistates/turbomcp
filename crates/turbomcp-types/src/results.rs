//! Result types for MCP operations.
//!
//! This module provides ergonomic result types for the three core MCP operations:
//! - `ToolResult` - Results from tool invocations
//! - `ResourceResult` - Results from resource reads
//! - `PromptResult` - Results from prompt retrieval

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::content::{Content, Message, Role};

/// Result from calling a tool.
///
/// This is the unified result type for all tool invocations. It supports:
/// - Simple text responses
/// - Error responses
/// - JSON/structured responses
/// - Multi-content responses (text + images, etc.)
///
/// # Examples
///
/// ```
/// use turbomcp_types::ToolResult;
///
/// // Simple text result
/// let result = ToolResult::text("Hello, world!");
///
/// // Error result
/// let error = ToolResult::error("Something went wrong");
///
/// // JSON result with structured content
/// let json = ToolResult::json(&serde_json::json!({"key": "value"})).unwrap();
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    /// Content blocks in the result
    pub content: Vec<Content>,
    /// Whether this result represents an error
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Structured content conforming to the tool's output schema
    ///
    /// Use this field when your tool has declared an `output_schema` in its Tool definition.
    /// The structured content should conform to that schema and provides machine-readable
    /// output for LLMs to parse programmatically.
    ///
    /// # When to Use
    ///
    /// - **Use `structured_content`**: When the tool returns data that should be parsed
    ///   by the LLM (JSON objects, arrays, typed data matching the output schema)
    /// - **Use `content`**: For human-readable text, error messages, logs, or unstructured output
    ///
    /// # Relationship to Tool::output_schema
    ///
    /// If your tool declares:
    /// ```rust,ignore
    /// Tool {
    ///     name: "get_user",
    ///     output_schema: Some(json!({
    ///         "type": "object",
    ///         "properties": {
    ///             "id": {"type": "number"},
    ///             "name": {"type": "string"}
    ///         }
    ///     }))
    /// }
    /// ```
    ///
    /// Then return structured content matching that schema:
    /// ```rust,ignore
    /// ToolResult::json(&json!({
    ///     "id": 42,
    ///     "name": "Alice"
    /// }))
    /// ```
    ///
    /// The `structured_content` field enables the LLM to extract typed data without parsing
    /// natural language from the `content` field.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

impl ToolResult {
    /// Create a text result.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::ToolResult;
    ///
    /// let result = ToolResult::text("Operation completed");
    /// assert!(!result.is_error());
    /// ```
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            is_error: None,
            structured_content: None,
        }
    }

    /// Create an error result.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::ToolResult;
    ///
    /// let result = ToolResult::error("File not found");
    /// assert!(result.is_error());
    /// ```
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            is_error: Some(true),
            structured_content: None,
        }
    }

    /// Create a JSON result with structured content.
    ///
    /// This creates both human-readable text content and machine-readable
    /// structured content for tools with output schemas.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::ToolResult;
    ///
    /// let data = serde_json::json!({"count": 42, "items": ["a", "b"]});
    /// let result = ToolResult::json(&data).unwrap();
    /// assert!(result.structured_content.is_some());
    /// ```
    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let structured = serde_json::to_value(value)?;
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self {
            content: vec![Content::text(text)],
            is_error: None,
            structured_content: Some(structured),
        })
    }

    /// Create an empty result (no content).
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if this result represents an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.is_error.unwrap_or(false)
    }

    /// Add structured content to the result.
    #[must_use]
    pub fn with_structured<T: Serialize>(mut self, value: &T) -> Self {
        self.structured_content = serde_json::to_value(value).ok();
        self
    }

    /// Add additional content to the result.
    #[must_use]
    pub fn with_content(mut self, content: Content) -> Self {
        self.content.push(content);
        self
    }

    /// Add image content to the result.
    #[must_use]
    pub fn with_image(self, data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        self.with_content(Content::image(data, mime_type))
    }

    /// Get the first text content if present.
    #[must_use]
    pub fn first_text(&self) -> Option<&str> {
        self.content.first().and_then(|c| c.as_text())
    }
}

/// Result from reading a resource.
///
/// # Examples
///
/// ```
/// use turbomcp_types::ResourceResult;
///
/// // Text resource
/// let result = ResourceResult::text("file:///example.txt", "File contents");
///
/// // JSON resource
/// let json = ResourceResult::json("config://settings", &serde_json::json!({"debug": true})).unwrap();
///
/// // Binary resource
/// let binary = ResourceResult::binary("file:///image.png", "base64data...", "image/png");
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourceResult {
    /// Resource contents (can be multiple for multi-part resources)
    pub contents: Vec<ResourceContent>,
}

impl ResourceResult {
    /// Create a text resource result.
    #[must_use]
    pub fn text(uri: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            contents: vec![ResourceContent {
                uri: uri.into(),
                mime_type: Some("text/plain".into()),
                text: Some(content.into()),
                blob: None,
            }],
        }
    }

    /// Create a JSON resource result.
    pub fn json<T: Serialize>(
        uri: impl Into<String>,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            contents: vec![ResourceContent {
                uri: uri.into(),
                mime_type: Some("application/json".into()),
                text: Some(serde_json::to_string_pretty(value)?),
                blob: None,
            }],
        })
    }

    /// Create a binary resource result.
    #[must_use]
    pub fn binary(
        uri: impl Into<String>,
        data: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            contents: vec![ResourceContent {
                uri: uri.into(),
                mime_type: Some(mime_type.into()),
                text: None,
                blob: Some(data.into()),
            }],
        }
    }

    /// Create an empty resource result.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add additional content to the result.
    #[must_use]
    pub fn with_content(mut self, content: ResourceContent) -> Self {
        self.contents.push(content);
        self
    }

    /// Get the first content's text if present.
    #[must_use]
    pub fn first_text(&self) -> Option<&str> {
        self.contents.first().and_then(|c| c.text.as_deref())
    }
}

/// Content of a resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceContent {
    /// Resource URI
    pub uri: String,
    /// MIME type of the content
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text content (for text resources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Binary content as base64 (for binary resources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

impl ResourceContent {
    /// Create text content.
    #[must_use]
    pub fn text(uri: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some("text/plain".into()),
            text: Some(content.into()),
            blob: None,
        }
    }

    /// Create binary content.
    #[must_use]
    pub fn binary(
        uri: impl Into<String>,
        data: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some(mime_type.into()),
            text: None,
            blob: Some(data.into()),
        }
    }
}

/// Result from getting a prompt.
///
/// # Examples
///
/// ```
/// use turbomcp_types::PromptResult;
///
/// // Simple user prompt
/// let result = PromptResult::user("Hello! How can I help?");
///
/// // Multi-turn prompt
/// let result = PromptResult::user("What's the weather like?")
///     .add_assistant("I'd be happy to help. What location?")
///     .add_user("San Francisco")
///     .with_description("Weather inquiry prompt");
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptResult {
    /// Description of this prompt result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Messages in the prompt
    pub messages: Vec<Message>,
}

impl PromptResult {
    /// Create a prompt result with messages.
    #[must_use]
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            description: None,
            messages,
        }
    }

    /// Create a prompt with a single user message.
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(vec![Message::user(text)])
    }

    /// Create a prompt with a single assistant message.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(vec![Message::assistant(text)])
    }

    /// Create an empty prompt.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add a description to the prompt.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a user message to the prompt.
    #[must_use]
    pub fn add_user(mut self, text: impl Into<String>) -> Self {
        self.messages.push(Message::user(text));
        self
    }

    /// Add an assistant message to the prompt.
    #[must_use]
    pub fn add_assistant(mut self, text: impl Into<String>) -> Self {
        self.messages.push(Message::assistant(text));
        self
    }

    /// Add a message with a specific role.
    #[must_use]
    pub fn add_message(mut self, role: Role, text: impl Into<String>) -> Self {
        self.messages.push(Message::new(role, Content::text(text)));
        self
    }

    /// Check if the prompt is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get the number of messages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_text() {
        let result = ToolResult::text("Hello");
        assert!(!result.is_error());
        assert_eq!(result.first_text(), Some("Hello"));
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("Failed");
        assert!(result.is_error());
        assert_eq!(result.first_text(), Some("Failed"));
    }

    #[test]
    fn test_tool_result_json() {
        let data = serde_json::json!({"key": "value"});
        let result = ToolResult::json(&data).unwrap();
        assert!(result.structured_content.is_some());
        assert!(!result.is_error());
    }

    #[test]
    fn test_resource_result_text() {
        let result = ResourceResult::text("file:///test.txt", "content");
        assert_eq!(result.first_text(), Some("content"));
        assert_eq!(result.contents[0].uri, "file:///test.txt");
    }

    #[test]
    fn test_resource_result_binary() {
        let result = ResourceResult::binary("file:///img.png", "base64data", "image/png");
        assert_eq!(result.contents[0].blob, Some("base64data".into()));
        assert_eq!(result.contents[0].mime_type, Some("image/png".into()));
    }

    #[test]
    fn test_prompt_result_builder() {
        let result = PromptResult::user("Hello")
            .add_assistant("Hi there")
            .add_user("How are you?")
            .with_description("Greeting");

        assert_eq!(result.len(), 3);
        assert_eq!(result.description, Some("Greeting".into()));
        assert!(result.messages[0].is_user());
        assert!(result.messages[1].is_assistant());
        assert!(result.messages[2].is_user());
    }

    #[test]
    fn test_prompt_result_serde() {
        let result = PromptResult::user("Test").with_description("A test prompt");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: PromptResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, parsed);
    }
}
