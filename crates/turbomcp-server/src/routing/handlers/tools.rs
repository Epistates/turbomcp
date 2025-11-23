//! Tool handlers for MCP tool operations - PURE BUSINESS LOGIC ONLY
//!
//! This module contains only the core business logic for tool operations.
//! All cross-cutting concerns (logging, timeout, performance) are handled by middleware.

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{CallToolRequest, ListToolsResult},
};

use super::HandlerContext;
use crate::ServerError;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle list tools request
pub async fn handle_list(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    let tools = context.registry.get_tool_definitions();
    let result = ListToolsResult {
        tools,
        next_cursor: None,
        _meta: None,
    };
    success_response(&request, result)
}

/// Handle call tool request - pure business logic only
pub async fn handle_call(
    context: &HandlerContext,
    request: JsonRpcRequest,
    ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<CallToolRequest>(&request) {
        Ok(call_request) => {
            let tool_name = call_request.name.clone();

            if let Some(handler) = context.registry.get_tool(&tool_name) {
                // Check if task augmentation is requested (MCP 2025-11-25 draft - SEP-1686)
                #[cfg(feature = "mcp-tasks")]
                let task_id = if let Some(task_metadata) = &call_request.task {
                    // Create task before executing tool
                    match context
                        .task_storage
                        .create_task(task_metadata.clone(), None)
                    {
                        Ok(id) => {
                            // Update task to Working status
                            let _ = context.task_storage.update_status(
                                &id,
                                turbomcp_protocol::types::TaskStatus::Working,
                                Some(format!("Executing tool: {}", tool_name)),
                                None,
                            );
                            Some(id)
                        }
                        Err(e) => {
                            return error_response(&request, e);
                        }
                    }
                } else {
                    None
                };

                // Execute the tool
                match handler.handle(call_request, ctx).await {
                    Ok(tool_result) => {
                        // If task was created, update it with the result
                        #[cfg(feature = "mcp-tasks")]
                        if let Some(ref task_id) = task_id {
                            // Complete the task with the tool result
                            let _ = context.task_storage.complete_task(
                                task_id,
                                serde_json::to_value(&tool_result).unwrap_or(serde_json::json!({})),
                                None,
                            );
                            // Add task_id to the tool result metadata
                            tool_result.task_id = Some(task_id.clone());
                        }

                        success_response(&request, tool_result)
                    }
                    Err(e) => {
                        // If task was created, mark it as failed
                        #[cfg(feature = "mcp-tasks")]
                        if let Some(ref task_id) = task_id {
                            let _ = context.task_storage.fail_task(task_id, e.to_string(), None);
                        }

                        error_response(&request, e)
                    }
                }
            } else {
                let error = ServerError::not_found(format!("Tool '{tool_name}'"));
                error_response(&request, error)
            }
        }
        Err(e) => error_response(&request, e),
    }
}
