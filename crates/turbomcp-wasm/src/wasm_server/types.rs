//! Types for Cloudflare Workers MCP handlers
//!
//! This module re-exports canonical types from `turbomcp-types` to ensure
//! a single source of truth across the TurboMCP ecosystem.
//!
//! ## DRY Architecture
//!
//! Result types (`ResourceResult`, `PromptResult`) are re-exported from
//! `turbomcp-types` rather than duplicated. This ensures:
//! - Consistent behavior across native and WASM targets
//! - Single point of maintenance for type definitions
//! - Type compatibility between crates
//!
//! ## Note on ToolResult
//!
//! `ToolResult` is aliased from `turbomcp_core::types::tools::CallToolResult`
//! because the WASM handler machinery uses `IntoToolResponse` which returns
//! `CallToolResult`. This is the internal wire format type. Users can treat
//! it identically to `turbomcp_types::ToolResult` for common operations.

use serde::{Deserialize, Serialize};

// Re-export result types from turbomcp-types (single source of truth)
pub use turbomcp_types::{PromptResult, ResourceResult};

// ToolResult is aliased from CallToolResult for IntoToolResponse compatibility
// This is required because the handler traits return CallToolResult
pub use turbomcp_core::types::tools::CallToolResult as ToolResult;

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

/// Standard JSON-RPC error codes (re-exported from core - single source of truth)
pub(crate) use turbomcp_core::error_codes;

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_types::Role;

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
        assert_eq!(result.contents[0].uri(), "file:///test");
        assert_eq!(result.contents[0].text(), Some("content"));
    }

    #[test]
    fn test_resource_result_binary() {
        let result = ResourceResult::binary("file:///img", "base64data", "image/png");
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].blob(), Some("base64data"));
        assert_eq!(result.contents[0].mime_type(), Some("image/png"));
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
