//! Helper functions and utilities

use crate::{CallToolResult, Content, GetPromptResult, TextContent};

/// Create text content helper
pub fn text<S: AsRef<str>>(content: S) -> Content {
    Content::Text(TextContent {
        text: content.as_ref().to_string(),
        annotations: None,
        meta: None,
    })
}

/// Create an error content helper  
pub fn error_text<S: AsRef<str>>(message: S) -> Content {
    Content::Text(TextContent {
        text: format!("Error: {}", message.as_ref()),
        annotations: None,
        meta: None,
    })
}

/// Create a successful tool result
#[must_use]
pub const fn tool_success(content: Vec<Content>) -> CallToolResult {
    CallToolResult {
        content,
        is_error: Some(false),
    }
}

/// Create an error tool result
pub fn tool_error<S: AsRef<str>>(message: S) -> CallToolResult {
    CallToolResult {
        content: vec![error_text(message)],
        is_error: Some(true),
    }
}

/// Create a prompt result with description
pub fn prompt_result<S: AsRef<str>>(
    content: S,
    description: S,
) -> crate::McpResult<GetPromptResult> {
    use turbomcp_protocol::types::{PromptMessage, Role};

    Ok(GetPromptResult {
        messages: vec![PromptMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: content.as_ref().to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        description: Some(description.as_ref().to_string()),
    })
}

/// Create a resource read result
pub fn resource_result<S: AsRef<str>>(
    content: S,
) -> crate::McpResult<turbomcp_protocol::types::ReadResourceResult> {
    use turbomcp_protocol::types::{ReadResourceResult, ResourceContent, TextResourceContents};

    Ok(ReadResourceResult {
        contents: vec![ResourceContent::Text(TextResourceContents {
            uri: "text://content".to_string(),
            mime_type: Some("text/plain".to_string()),
            text: content.as_ref().to_string(),
            meta: None,
        })],
    })
}
