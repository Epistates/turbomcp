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
use crate::McpError;
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
                if let Some(task_metadata) = call_request.task.clone() {
                    // Create task before executing tool
                    match context.task_storage.create_task(task_metadata, None) {
                        Ok(task_id) => {
                            // Update task to Working status
                            let _ = context.task_storage.update_status(
                                &task_id,
                                turbomcp_protocol::types::TaskStatus::Working,
                                Some(format!("Executing tool: {}", tool_name)),
                                None,
                            );

                            // Spawn background task for execution
                            let handler = handler.clone();
                            let ctx = ctx.clone();
                            let task_storage = context.task_storage.clone();
                            let task_id_clone = task_id.clone();
                            let call_request_clone = call_request.clone();

                            tokio::spawn(async move {
                                match handler.handle(call_request_clone, ctx).await {
                                    Ok(mut tool_result) => {
                                        // Add task_id to the tool result metadata
                                        tool_result.task_id = Some(task_id_clone.clone());

                                        // Complete the task with the tool result
                                        let _ = task_storage.complete_task(
                                            &task_id_clone,
                                            serde_json::to_value(&tool_result)
                                                .unwrap_or(serde_json::json!({})),
                                            None,
                                        );
                                    }
                                    Err(e) => {
                                        let _ = task_storage.fail_task(
                                            &task_id_clone,
                                            e.to_string(),
                                            None,
                                        );
                                    }
                                }
                            });

                            // Return CreateTaskResult immediately
                            // We need to fetch the created task to return it
                            let task = context
                                .task_storage
                                .get_task(&task_id, None)
                                .unwrap_or_else(|_| {
                                    // Fallback if somehow deleted immediately (unlikely)
                                    turbomcp_protocol::types::tasks::Task {
                                        task_id: task_id.clone(),
                                        status: turbomcp_protocol::types::TaskStatus::Working,
                                        status_message: Some("Task created".to_string()),
                                        created_at: chrono::Utc::now().to_rfc3339(),
                                        last_updated_at: chrono::Utc::now().to_rfc3339(),
                                        ttl: None,
                                        poll_interval: None,
                                    }
                                });

                            let result = turbomcp_protocol::types::tasks::CreateTaskResult {
                                task,
                                _meta: None,
                            };

                            return success_response(&request, result);
                        }
                        Err(e) => {
                            return error_response(&request, e);
                        }
                    }
                }

                // Normal execution (synchronous / no task)
                match handler.handle(call_request, ctx).await {
                    #[cfg(feature = "mcp-tasks")]
                    Ok(tool_result) => success_response(&request, tool_result),
                    #[cfg(not(feature = "mcp-tasks"))]
                    Ok(tool_result) => success_response(&request, tool_result),
                    Err(e) => error_response(&request, e),
                }
            } else {
                let error = McpError::not_found(format!("Tool '{tool_name}'"));
                error_response(&request, error)
            }
        }
        Err(e) => error_response(&request, e),
    }
}
