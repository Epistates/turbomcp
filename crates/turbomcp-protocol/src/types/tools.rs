//! Types for the MCP tool-calling system.
//!
//! Tool definition, schema, annotation, and execution types are canonically
//! defined in [`turbomcp_types`] and re-exported here. This module contains
//! only the protocol-level wire wrappers (`ListToolsRequest`, `ListToolsResult`,
//! `CallToolRequest`, `CallToolResult`) plus helpers on them.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{content::ContentBlock, core::Cursor};

/// Re-exports of the canonical tool-definition types from [`turbomcp_types`].
pub use turbomcp_types::{
    TaskSupportLevel as TaskSupportMode, Tool, ToolAnnotations, ToolExecution, ToolInputSchema,
    ToolOutputSchema,
};

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
///
/// ## Version Support
/// - MCP 2025-11-25: name, arguments, _meta
/// - MCP 2025-11-25 draft (SEP-1686): + task (optional task augmentation)
///
/// ## Task Augmentation
///
/// When the `task` field is present, the receiver responds immediately with
/// a `CreateTaskResult` containing a task ID. The actual tool result is available
/// later via `tasks/result`.
///
/// ```rust,ignore
/// use turbomcp_protocol::types::{CallToolRequest, tasks::TaskMetadata};
///
/// let request = CallToolRequest {
///     name: "long_running_tool".to_string(),
///     arguments: Some(json!({"data": "value"})),
///     task: Some(TaskMetadata { ttl: Some(300_000) }), // 5 minute lifetime
///     _meta: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CallToolRequest {
    /// The programmatic name of the tool to call.
    pub name: String,

    /// The arguments to pass to the tool, conforming to its `input_schema`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, serde_json::Value>>,

    /// Optional task metadata for task-augmented requests (MCP 2025-11-25 draft)
    ///
    /// When present, this request will be executed asynchronously and the receiver
    /// will respond immediately with a `CreateTaskResult`. The actual tool result
    /// is available later via `tasks/result`.
    ///
    /// Requires:
    /// - Server capability: `tasks.requests.tools.call`
    /// - Tool annotation: `taskHint` must be "optional" or "always" (or absent/"never" for default)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<crate::types::tasks::TaskMetadata>,

    /// Optional metadata for the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// The result of a `CallToolRequest`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// Optional task ID when tool execution is augmented with task tracking (MCP 2025-11-25 draft - SEP-1686).
    ///
    /// When a tool call includes task metadata, the server creates a task to track the operation
    /// and returns the task_id here. Clients can use this to monitor progress via tasks/get
    /// or retrieve final results via tasks/result.
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl CallToolResult {
    /// Create a successful result with a single text content block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text(turbomcp_types::TextContent {
                text: text.into(),
                annotations: None,
                meta: None,
            })],
            is_error: None,
            structured_content: None,
            _meta: None,
            task_id: None,
        }
    }

    /// Create an error result with a single text content block and `is_error = true`.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text(turbomcp_types::TextContent {
                text: message.into(),
                annotations: None,
                meta: None,
            })],
            is_error: Some(true),
            structured_content: None,
            _meta: None,
            task_id: None,
        }
    }

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
    ///     task_id: None,
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
    ///     task_id: None,
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
    ///     task_id: None,
    /// };
    /// assert!(!success_result.has_error());
    ///
    /// let error_result = CallToolResult {
    ///     content: vec![],
    ///     is_error: Some(true),
    ///     structured_content: None,
    ///     _meta: None,
    ///     task_id: None,
    /// };
    /// assert!(error_result.has_error());
    ///
    /// let unspecified_result = CallToolResult {
    ///     content: vec![],
    ///     is_error: None,
    ///     structured_content: None,
    ///     _meta: None,
    ///     task_id: None,
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
    ///     task_id: None,
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
