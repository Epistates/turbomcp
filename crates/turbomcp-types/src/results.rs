//! Result types for MCP operations.
//!
//! This module provides ergonomic result types for the three core MCP operations:
//! - `ToolResult` - Results from tool invocations
//! - `ResourceResult` - Results from resource reads
//! - `PromptResult` - Results from prompt retrieval

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::content::{
    BlobResourceContents, Content, Message, ResourceContents, Role, TextResourceContents,
};

/// Result from calling a tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    /// Content blocks in the result
    pub content: Vec<Content>,
    /// Whether this result represents an error
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Structured content conforming to the tool's output schema
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl ToolResult {
    /// Create a text result.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            ..Default::default()
        }
    }

    /// Create an error result.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            is_error: Some(true),
            ..Default::default()
        }
    }

    /// Create a JSON result with structured content.
    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let structured = serde_json::to_value(value)?;
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self {
            content: vec![Content::text(text)],
            structured_content: Some(structured),
            ..Default::default()
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

    /// Set metadata.
    #[must_use]
    pub fn with_meta(mut self, meta: HashMap<String, Value>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Get the first text content if present.
    #[must_use]
    pub fn first_text(&self) -> Option<&str> {
        self.content.first().and_then(|c| c.as_text())
    }
}

/// Result from reading a resource.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourceResult {
    /// Resource contents
    pub contents: Vec<ResourceContents>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl ResourceResult {
    /// Create a text resource result.
    #[must_use]
    pub fn text(uri: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            contents: vec![ResourceContents::Text(TextResourceContents {
                uri: uri.into(),
                mime_type: Some("text/plain".into()),
                text: content.into(),
            })],
            ..Default::default()
        }
    }

    /// Create a JSON resource result.
    pub fn json<T: Serialize>(
        uri: impl Into<String>,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            contents: vec![ResourceContents::Text(TextResourceContents {
                uri: uri.into(),
                mime_type: Some("application/json".into()),
                text: serde_json::to_string_pretty(value)?,
            })],
            ..Default::default()
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
            contents: vec![ResourceContents::Blob(BlobResourceContents {
                uri: uri.into(),
                mime_type: Some(mime_type.into()),
                blob: data.into(),
            })],
            ..Default::default()
        }
    }

    /// Create an empty resource result.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add additional content to the result.
    #[must_use]
    pub fn with_content(mut self, content: ResourceContents) -> Self {
        self.contents.push(content);
        self
    }

    /// Set metadata.
    #[must_use]
    pub fn with_meta(mut self, meta: HashMap<String, Value>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Get the first content's text if present.
    #[must_use]
    pub fn first_text(&self) -> Option<&str> {
        self.contents.first().and_then(|c| match c {
            ResourceContents::Text(t) => Some(t.text.as_str()),
            ResourceContents::Blob(_) => None,
        })
    }
}

/// Result from getting a prompt.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptResult {
    /// Description of this prompt result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Messages in the prompt
    pub messages: Vec<Message>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl PromptResult {
    /// Create a prompt result with messages.
    #[must_use]
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            ..Default::default()
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

    /// Set metadata.
    #[must_use]
    pub fn with_meta(mut self, meta: HashMap<String, Value>) -> Self {
        self.meta = Some(meta);
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
        match &result.contents[0] {
            ResourceContents::Text(t) => assert_eq!(t.uri, "file:///test.txt"),
            _ => panic!("Expected text resource contents"),
        }
    }

    #[test]
    fn test_resource_result_binary() {
        let result = ResourceResult::binary("file:///img.png", "base64data", "image/png");
        match &result.contents[0] {
            ResourceContents::Blob(b) => {
                assert_eq!(b.blob, "base64data");
                assert_eq!(b.mime_type, Some("image/png".into()));
            }
            _ => panic!("Expected blob resource contents"),
        }
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
