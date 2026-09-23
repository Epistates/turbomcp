//! Types for Cloudflare Workers MCP handlers.
//!
//! Re-exports canonical types from [`turbomcp_types`] to keep a single source
//! of truth across the TurboMCP ecosystem.
//!
//! ## Note on `ToolResult`
//!
//! `ToolResult` is aliased from [`turbomcp_types::CallToolResult`] because the
//! WASM handler machinery uses `IntoToolResponse`, which returns
//! `CallToolResult`. Users can treat it identically to
//! [`turbomcp_types::ToolResult`] for common operations.

// Re-export result types from turbomcp-types (single source of truth).
pub use turbomcp_types::{PromptResult, ResourceResult};

// ToolResult is aliased from CallToolResult for IntoToolResponse compat.
pub use turbomcp_types::CallToolResult as ToolResult;

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
}
