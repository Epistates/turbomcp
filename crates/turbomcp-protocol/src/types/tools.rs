//! Types for the MCP tool-calling system.
//!
//! Tool definition, schema, annotation, and execution types — along with the
//! `CallToolResult` wire wrapper — are canonically defined in [`turbomcp_types`]
//! and re-exported here. Protocol-local wrappers (`ListToolsRequest`,
//! `ListToolsResult`, `CallToolRequest`) remain here because they reference
//! the protocol's `Cursor` and task-augmentation types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::core::Cursor;

/// Re-exports of the canonical tool types from [`turbomcp_types`].
pub use turbomcp_types::{
    CallToolResult, TaskSupportLevel as TaskSupportMode, Tool, ToolAnnotations, ToolExecution,
    ToolInputSchema, ToolOutputSchema,
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
