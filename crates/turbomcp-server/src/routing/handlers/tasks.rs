//! Task handlers for MCP Tasks API (SEP-1686)
//!
//! Implements the four task API handlers:
//! - tasks/get: Retrieve task status
//! - tasks/result: Get task result (blocks until terminal state)
//! - tasks/list: List all tasks
//! - tasks/cancel: Cancel a running task

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{CancelTaskRequest, GetTaskRequest, ListTasksRequest, ListTasksResult},
};

use super::HandlerContext;
use crate::error::ServerError;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle tasks/get request - retrieve task status
pub async fn handle_get(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<GetTaskRequest>(&request) {
        Ok(get_request) => {
            // Extract task_id from request
            let task_id = &get_request.task_id;

            // Get task from storage (GetTaskResult is a type alias for Task)
            match context.task_storage.get_task(task_id, None) {
                Ok(task) => success_response(&request, task),
                Err(e) => error_response(&request, e),
            }
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle tasks/result request - get task result (blocks until terminal state)
pub async fn handle_result(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<GetTaskRequest>(&request) {
        Ok(get_request) => {
            let task_id = &get_request.task_id;

            // Block until task reaches terminal state
            match context.task_storage.get_task_result(task_id, None).await {
                Ok(result_state) => {
                    use crate::task_storage::TaskResultState;

                    match result_state {
                        TaskResultState::Completed(value) => success_response(&request, value),
                        TaskResultState::Failed(error_msg) => error_response(
                            &request,
                            ServerError::Lifecycle(format!("Task failed: {}", error_msg)),
                        ),
                        TaskResultState::Cancelled => error_response(
                            &request,
                            ServerError::Lifecycle("Task was cancelled".to_string()),
                        ),
                        TaskResultState::Pending => {
                            // Should never happen since get_task_result blocks
                            error_response(
                                &request,
                                ServerError::Lifecycle(
                                    "Task still pending (unexpected)".to_string(),
                                ),
                            )
                        }
                    }
                }
                Err(e) => error_response(&request, e),
            }
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle tasks/list request - list all tasks
pub async fn handle_list(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ListTasksRequest>(&request) {
        Ok(_list_request) => {
            // List all tasks (no auth filtering for now, no pagination)
            match context.task_storage.list_tasks(None) {
                Ok(tasks) => {
                    let result = ListTasksResult {
                        tasks,
                        next_cursor: None,
                        _meta: None,
                    };
                    success_response(&request, result)
                }
                Err(e) => error_response(&request, e),
            }
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle tasks/cancel request - cancel a running task
pub async fn handle_cancel(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<CancelTaskRequest>(&request) {
        Ok(cancel_request) => {
            let task_id = &cancel_request.task_id;

            // Cancel the task
            match context.task_storage.cancel_task(task_id, None, None) {
                Ok(()) => {
                    // Get the updated task to return (CancelTaskResult is a type alias for Task)
                    match context.task_storage.get_task(task_id, None) {
                        Ok(task) => success_response(&request, task),
                        Err(e) => error_response(&request, e),
                    }
                }
                Err(e) => error_response(&request, e),
            }
        }
        Err(e) => error_response(&request, e),
    }
}
