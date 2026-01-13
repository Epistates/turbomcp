//! Types for Cloudflare Workers MCP handlers

use serde::{Deserialize, Serialize};
use turbomcp_core::types::content::{Content, PromptMessage};
use turbomcp_core::types::core::Role;

/// Result from a tool execution
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    /// Content returned by the tool
    pub content: Vec<Content>,
    /// Whether the tool execution resulted in an error
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ToolResult {
    /// Create a successful text result
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text {
                text: text.into(),
                annotations: None,
            }],
            is_error: None,
        }
    }

    /// Create a successful JSON result
    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self::text(text))
    }

    /// Create an error result
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text {
                text: message.into(),
                annotations: None,
            }],
            is_error: Some(true),
        }
    }

    /// Create a result with multiple content items
    pub fn contents(contents: Vec<Content>) -> Self {
        Self {
            content: contents,
            is_error: None,
        }
    }

    /// Create an image result (base64 encoded)
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Image {
                data: data.into(),
                mime_type: mime_type.into(),
                annotations: None,
            }],
            is_error: None,
        }
    }
}

/// Result from reading a resource
#[derive(Debug, Clone, Serialize)]
pub struct ResourceResult {
    /// Contents of the resource
    pub contents: Vec<ResourceContent>,
}

/// Content of a resource
#[derive(Debug, Clone, Serialize)]
pub struct ResourceContent {
    /// URI of the resource
    pub uri: String,
    /// MIME type of the content
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text content (mutually exclusive with blob)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Binary content as base64 (mutually exclusive with text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

impl ResourceResult {
    /// Create a text resource result
    pub fn text(uri: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            contents: vec![ResourceContent {
                uri: uri.into(),
                mime_type: Some("text/plain".to_string()),
                text: Some(content.into()),
                blob: None,
            }],
        }
    }

    /// Create a JSON resource result
    pub fn json<T: Serialize>(
        uri: impl Into<String>,
        value: &T,
    ) -> Result<Self, serde_json::Error> {
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self {
            contents: vec![ResourceContent {
                uri: uri.into(),
                mime_type: Some("application/json".to_string()),
                text: Some(text),
                blob: None,
            }],
        })
    }

    /// Create a binary resource result
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
}

/// Result from getting a prompt
#[derive(Debug, Clone, Serialize)]
pub struct PromptResult {
    /// Description of the prompt result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Messages in the prompt
    pub messages: Vec<PromptMessage>,
}

impl PromptResult {
    /// Create a simple user prompt
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            description: None,
            messages: vec![PromptMessage {
                role: Role::User,
                content: Content::text(text),
            }],
        }
    }

    /// Create a simple assistant prompt
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            description: None,
            messages: vec![PromptMessage {
                role: Role::Assistant,
                content: Content::text(text),
            }],
        }
    }

    /// Create a prompt with description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Create a prompt with multiple messages
    pub fn messages(messages: Vec<PromptMessage>) -> Self {
        Self {
            description: None,
            messages,
        }
    }

    /// Add a user message to the prompt
    pub fn add_user(mut self, text: impl Into<String>) -> Self {
        self.messages.push(PromptMessage {
            role: Role::User,
            content: Content::text(text),
        });
        self
    }

    /// Add an assistant message to the prompt
    pub fn add_assistant(mut self, text: impl Into<String>) -> Self {
        self.messages.push(PromptMessage {
            role: Role::Assistant,
            content: Content::text(text),
        });
        self
    }
}

/// JSON-RPC request structure
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC response structure
#[derive(Debug, Clone, Serialize)]
pub(crate) struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<serde_json::Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// JSON-RPC error structure
#[derive(Debug, Clone, Serialize)]
pub(crate) struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Standard JSON-RPC error codes
pub(crate) mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_text() {
        let result = ToolResult::text("hello");
        assert_eq!(result.content.len(), 1);
        assert!(result.is_error.is_none());
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("something went wrong");
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_tool_result_json() {
        let data = serde_json::json!({"key": "value"});
        let result = ToolResult::json(&data).unwrap();
        assert_eq!(result.content.len(), 1);
    }

    #[test]
    fn test_resource_result_text() {
        let result = ResourceResult::text("file:///test", "content");
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].uri, "file:///test");
        assert_eq!(result.contents[0].text, Some("content".to_string()));
    }

    #[test]
    fn test_resource_result_binary() {
        let result = ResourceResult::binary("file:///img", "base64data", "image/png");
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].blob, Some("base64data".to_string()));
        assert_eq!(result.contents[0].mime_type, Some("image/png".to_string()));
    }

    #[test]
    fn test_prompt_result_user() {
        let result = PromptResult::user("Hello");
        assert_eq!(result.messages.len(), 1);
        assert!(matches!(result.messages[0].role, Role::User));
    }

    #[test]
    fn test_prompt_result_assistant() {
        let result = PromptResult::assistant("Hi there");
        assert_eq!(result.messages.len(), 1);
        assert!(matches!(result.messages[0].role, Role::Assistant));
    }

    #[test]
    fn test_prompt_result_builder() {
        let result = PromptResult::user("User message")
            .add_assistant("Assistant response")
            .add_user("Follow up")
            .with_description("A conversation");

        assert_eq!(result.messages.len(), 3);
        assert_eq!(result.description, Some("A conversation".to_string()));
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(Some(serde_json::json!(1)), serde_json::json!({}));
        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let response =
            JsonRpcResponse::error(Some(serde_json::json!(1)), -32600, "Invalid request");
        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32600);
        assert_eq!(error.message, "Invalid request");
    }
}
