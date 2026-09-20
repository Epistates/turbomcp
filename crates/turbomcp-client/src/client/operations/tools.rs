//! Tool operations for MCP client
//!
//! This module provides tool-related functionality including listing tools,
//! calling tools, and processing tool results.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, CreateTaskResult, Cursor, ListToolsRequest, ListToolsResult,
    TaskMetadata, Tool,
};
use turbomcp_protocol::{Error, Result};

/// Maximum number of pagination pages to prevent infinite loops from misbehaving servers.
const MAX_PAGINATION_PAGES: usize = 1000;

/// Response shape for `tools/call`.
#[derive(Debug, Clone)]
pub enum CallToolResponse {
    /// Immediate tool result.
    Result(CallToolResult),
    /// Task handle for task-augmented execution.
    Task(CreateTaskResult),
}

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
    /// List all available tools from the MCP server
    ///
    /// Returns complete tool definitions with schemas that can be used
    /// for form generation, validation, and documentation. Tools represent
    /// executable functions provided by the server.
    ///
    /// # Returns
    ///
    /// Returns a vector of Tool objects with complete metadata including names,
    /// descriptions, and input schemas. These schemas can be used to generate
    /// user interfaces for tool invocation.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let tools = client.list_tools().await?;
    /// for tool in tools {
    ///     println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or("No description"));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let mut all_tools = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGINATION_PAGES {
            let result = self.list_tools_paginated(cursor.clone()).await?;
            all_tools.extend(result.tools);
            match result.next_cursor {
                // `nextCursor` is the only end-of-results signal the spec
                // defines. An empty page that still carries one is legal — a
                // server may filter a page down to nothing — and stopping
                // there silently truncated the list. The second guard catches
                // a server that repeats a cursor forever, which would
                // otherwise spin to the page cap.
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        Ok(all_tools)
    }

    /// List tools with pagination support
    ///
    /// Returns the full `ListToolsResult` including `next_cursor` for manual
    /// pagination control. Use `list_tools()` for automatic pagination.
    ///
    /// # Arguments
    ///
    /// * `cursor` - Optional cursor from a previous `ListToolsResult::next_cursor`
    pub async fn list_tools_paginated(&self, cursor: Option<Cursor>) -> Result<ListToolsResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let request = ListToolsRequest {
            cursor,
            _meta: None,
        };
        let params = if request.cursor.is_some() {
            Some(serde_json::to_value(&request)?)
        } else {
            None
        };
        self.inner.protocol.request("tools/list", params).await
    }

    /// List available tool names from the MCP server
    ///
    /// Returns only the tool names for cases where full schemas are not needed.
    /// For most use cases, prefer `list_tools()` which provides complete tool definitions.
    ///
    /// # Returns
    ///
    /// Returns a vector of tool names available on the server.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let tool_names = client.list_tool_names().await?;
    /// for name in tool_names {
    ///     println!("Available tool: {}", name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tool_names(&self) -> Result<Vec<String>> {
        let tools = self.list_tools().await?;
        Ok(tools.into_iter().map(|tool| tool.name).collect())
    }

    /// Call a tool on the server
    ///
    /// Executes a tool on the server with the provided arguments and returns
    /// the complete MCP `CallToolResult`.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `arguments` - Optional arguments to pass to the tool
    /// * `task` - Must be `None`; task-augmented calls return `CreateTaskResult`.
    ///   Use [`Self::call_tool_task`] or [`Self::call_tool_response`] for task
    ///   execution.
    ///
    /// # Returns
    ///
    /// Returns the complete `CallToolResult` with:
    /// - `content: Vec<ContentBlock>` - All content blocks (text, image, resource, audio, etc.)
    /// - `is_error: Option<bool>` - Whether the tool execution resulted in an error
    /// - `structured_content: Option<serde_json::Value>` - Schema-validated structured output
    /// - `_meta: Option<serde_json::Value>` - Metadata for client applications (not exposed to LLMs)
    ///
    /// # Examples
    ///
    /// ## Basic Usage
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::ContentBlock;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let mut args = HashMap::new();
    /// args.insert("input".to_string(), serde_json::json!("test"));
    ///
    /// let result = client.call_tool("my_tool", Some(args), None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
        task: Option<TaskMetadata>,
    ) -> Result<CallToolResult> {
        if task.is_some() {
            return Err(Error::invalid_request(
                "task-augmented tools/call returns CreateTaskResult; use call_tool_task or call_tool_response",
            ));
        }

        match self.call_tool_response(name, arguments, None).await? {
            CallToolResponse::Result(result) => Ok(result),
            CallToolResponse::Task(_) => Err(Error::invalid_request(
                "task-augmented tools/call returned CreateTaskResult; use call_tool_task or call_tool_response",
            )),
        }
    }

    /// Call a tool and preserve the spec-level response variant.
    ///
    /// MCP 2025-11-25 task-augmented `tools/call` returns `CreateTaskResult`
    /// immediately. Non-task calls return `CallToolResult`.
    pub async fn call_tool_response(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
        task: Option<TaskMetadata>,
    ) -> Result<CallToolResponse> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        self.call_tool_response_with_progress(name, arguments, task, None)
            .await
    }

    /// `tools/call`, asking the server to report progress against `token`.
    ///
    /// The progress utility is opt-in from the *caller's* side: a server may
    /// only send `notifications/progress` for a token the client supplied in
    /// the request's `_meta`, so without this there was no way to receive any.
    /// Register a
    /// [`ProgressHandler`](crate::handlers::ProgressHandler) to see them.
    ///
    /// The token must be unique across this client's in-flight requests — it
    /// is what correlates a notification back to the call that produced it.
    pub async fn call_tool_response_with_progress(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
        task: Option<TaskMetadata>,
        progress_token: Option<serde_json::Value>,
    ) -> Result<CallToolResponse> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let is_task_augmented = task.is_some();
        let request_data = CallToolRequest {
            name: name.to_string(),
            arguments: Some(arguments.unwrap_or_default()),
            task,
            _meta: None,
        };

        let mut params = serde_json::to_value(&request_data)?;
        if let Some(token) = progress_token {
            // `_meta.progressToken` is where the spec puts it, on any request.
            params["_meta"] = serde_json::json!({ "progressToken": token });
        }

        let raw_result: serde_json::Value = self
            .inner
            .protocol
            .request("tools/call", Some(params))
            .await?;

        if is_task_augmented {
            serde_json::from_value(raw_result)
                .map(CallToolResponse::Task)
                .map_err(|e| {
                    Error::internal(format!("Failed to deserialize CreateTaskResult: {e}"))
                })
        } else {
            serde_json::from_value(raw_result)
                .map(CallToolResponse::Result)
                .map_err(|e| Error::internal(format!("Failed to deserialize CallToolResult: {e}")))
        }
    }

    /// Call a tool using MCP task-augmented execution.
    ///
    /// Returns the created task handle. Retrieve the final result with the
    /// Tasks API once the server reports completion.
    pub async fn call_tool_task(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
        task: TaskMetadata,
    ) -> Result<CreateTaskResult> {
        match self.call_tool_response(name, arguments, Some(task)).await? {
            CallToolResponse::Task(result) => Ok(result),
            CallToolResponse::Result(_) => Err(Error::invalid_request(
                "task-augmented tools/call returned CallToolResult instead of CreateTaskResult",
            )),
        }
    }
}
