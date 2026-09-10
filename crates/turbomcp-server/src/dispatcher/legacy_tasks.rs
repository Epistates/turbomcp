//! Core Tasks (`2025-11-25`): task-augmented `tools/call`, the `tasks/*`
//! methods, and per-tool `taskSupport` advertisement. The draft serves Tasks
//! as an extension instead (see [`super::augment`]).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};

use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpError,
    RequestContext, RequestId,
};
use turbomcp_protocol::methods;
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_service::mcp_to_jsonrpc_error;

use crate::context::{CallToolContext, ListToolsContext};
use crate::router::MethodRouter;
use crate::tasks::{TaskBackend, TaskError, TaskSnapshot, TaskStatus};
use crate::traits::McpServerCore;

use super::params::{parse_call_tool_params, parse_list_params};
use super::{error_response, ok_value, session_id};

// ---- core Tasks (2025-11-25) ---------------------------------------------------

/// How many tasks one `tasks/list` page carries.
const TASKS_PAGE_SIZE: usize = 50;

/// Whether a `tools/call` request asks for task-augmented execution. The
/// field's *shape* is validated in [`task_augmented_call`]; mere presence
/// routes there. (With Tasks disabled the field is ignored entirely and the
/// call processes normally, per spec §Task Support and Handling.)
pub(super) fn has_task_field(params: Option<&Value>) -> bool {
    params.and_then(|p| p.get("task")).is_some()
}

#[derive(Deserialize)]
struct RawTaskMetadata {
    #[serde(default)]
    ttl: Option<i64>,
}

/// `tools/call` with a `task` field: validate, register the task, spawn the
/// handler under the task's cancellation token, and answer immediately with
/// `CreateTaskResult` (spec §Creating Tasks).
pub(super) async fn task_augmented_call<S: McpServerCore>(
    server: S,
    router: &MethodRouter<S>,
    store: &Arc<dyn TaskBackend>,
    ctx: RequestContext,
    req: &JsonRpcRequest,
    id: RequestId,
    contract: (Arc<crate::catalog::Validators>, Option<Value>),
) -> JsonRpcMessage {
    let task_meta: RawTaskMetadata = match req
        .params
        .as_ref()
        .and_then(|p| p.get("task"))
        .map(|t| serde_json::from_value(t.clone()))
    {
        Some(Ok(m)) => m,
        _ => {
            return error_response(
                id,
                &McpError::invalid_params("invalid tools/call `task` augmentation"),
            );
        }
    };
    let params = match parse_call_tool_params(req.params.as_ref()) {
        Ok(p) => p,
        Err(e) => return error_response(id, &e),
    };

    // The task's token doubles as the handler's request cancellation, so
    // `tasks/cancel` (and ttl purge) reach a cooperative handler.
    let token = CancellationToken::new();
    let mut ctx = ctx;
    ctx.cancellation = token.clone();
    let Some(fut) = router.dispatch_call_tool(server, CallToolContext::new(ctx), params) else {
        return error_response(
            id,
            &McpError::method_not_found(methods::request::TOOLS_CALL),
        );
    };

    let fut = async move { contract.0.output(contract.1.as_ref(), fut.await?) };

    // The legacy gate guarantees a session id by the time we're here.
    let sid = session_id(req.params.as_ref())
        .unwrap_or_default()
        .to_owned();
    let snap = match store.create(&sid, task_meta.ttl, token.clone()).await {
        Ok(s) => s,
        Err(e) => return task_error_response(id, &e),
    };

    let poll_interval_ms = store.poll_interval_ms();
    let store = Arc::clone(store);
    let task_id = snap.task_id.clone();
    tokio::spawn(async move {
        tokio::select! {
            () = token.cancelled() => {
                // `tasks/cancel` (or expiry purge) already transitioned the
                // record; dropping `fut` aborts the handler.
            }
            out = fut => {
                let outcome = match out {
                    Ok(result) => {
                        serde_json::to_value(legacy::CallToolResult::from(result)).map_err(|e| {
                            JsonRpcError {
                                code: -32603,
                                message: format!("serialize result: {e}"),
                                data: None,
                            }
                        })
                    }
                    Err(e) => Err(mcp_to_jsonrpc_error(&e)),
                };
                store.complete(&task_id, outcome).await;
            }
        }
    });

    ok_value(
        id,
        &legacy::CreateTaskResult {
            meta: Map::new(),
            task: to_wire_task(&snap, poll_interval_ms),
        },
    )
}

/// Legacy `tools/list` with Tasks enabled: every tool that doesn't declare its
/// own task support is advertised as `execution.taskSupport: "optional"`
/// (the conversion layer can't know Tasks are on, so the dispatcher patches).
pub(super) async fn legacy_list_tools_with_task_support<S: McpServerCore>(
    server: S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: RequestContext,
    id: RequestId,
) -> JsonRpcMessage {
    let list_params = parse_list_params(req.params.as_ref());
    let Some(fut) = router.dispatch_list_tools(server, ListToolsContext::new(ctx), list_params)
    else {
        return error_response(
            id,
            &McpError::method_not_found(methods::request::TOOLS_LIST),
        );
    };
    match fut.await {
        Ok(result) => {
            let mut wire = legacy::ListToolsResult::from(result);
            // If any tool declared per-tool support (`#[tool(task)]`), honor those
            // and default the rest to `forbidden`. If none did, keep the blanket
            // `optional` default (backward-compatible with servers that only flip
            // Tasks on globally).
            let any_declared = wire
                .tools
                .iter()
                .any(|t| t.execution.as_ref().and_then(|e| e.task_support).is_some());
            let default = if any_declared {
                legacy::ToolExecutionTaskSupport::Forbidden
            } else {
                legacy::ToolExecutionTaskSupport::Optional
            };
            for tool in &mut wire.tools {
                tool.execution
                    .get_or_insert(legacy::ToolExecution { task_support: None })
                    .task_support
                    .get_or_insert(default);
            }
            ok_value(id, &wire)
        }
        Err(e) => error_response(id, &e),
    }
}

pub(super) async fn handle_tasks_method(
    store: &Arc<dyn TaskBackend>,
    sid: &str,
    method: &str,
    req: &JsonRpcRequest,
    id: RequestId,
) -> JsonRpcMessage {
    match method {
        methods::request::TASKS_LIST => {
            let cursor = req
                .params
                .as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(Value::as_str);
            match store.list(sid, cursor, TASKS_PAGE_SIZE).await {
                Ok((page, next_cursor)) => ok_value(
                    id,
                    &legacy::ListTasksResult {
                        meta: Map::new(),
                        next_cursor,
                        tasks: page
                            .iter()
                            .map(|s| to_wire_task(s, store.poll_interval_ms()))
                            .collect(),
                    },
                ),
                Err(e) => task_error_response(id, &e),
            }
        }
        methods::request::TASKS_GET => match parse_task_id(req.params.as_ref()) {
            Err(e) => error_response(id, &e),
            Ok(tid) => match store.get(sid, &tid).await {
                Ok(s) => ok_value(
                    id,
                    &legacy::GetTaskResult {
                        created_at: s.created_at.clone(),
                        last_updated_at: s.last_updated_at.clone(),
                        meta: Map::new(),
                        poll_interval: Some(store.poll_interval_ms()),
                        status: to_wire_status(s.status),
                        status_message: s.status_message.clone(),
                        task_id: s.task_id.clone(),
                        ttl: Some(s.ttl_ms),
                        extra: Map::new(),
                    },
                ),
                Err(e) => task_error_response(id, &e),
            },
        },
        methods::request::TASKS_CANCEL => match parse_task_id(req.params.as_ref()) {
            Err(e) => error_response(id, &e),
            Ok(tid) => match store.cancel(sid, &tid).await {
                Ok(s) => ok_value(
                    id,
                    &legacy::CancelTaskResult {
                        created_at: s.created_at.clone(),
                        last_updated_at: s.last_updated_at.clone(),
                        meta: Map::new(),
                        poll_interval: Some(store.poll_interval_ms()),
                        status: to_wire_status(s.status),
                        status_message: s.status_message.clone(),
                        task_id: s.task_id.clone(),
                        ttl: Some(s.ttl_ms),
                        extra: Map::new(),
                    },
                ),
                Err(e) => task_error_response(id, &e),
            },
        },
        methods::request::TASKS_RESULT => match parse_task_id(req.params.as_ref()) {
            Err(e) => error_response(id, &e),
            // Blocks until the task is terminal, then answers exactly what the
            // underlying request would have (spec §Result Retrieval).
            Ok(tid) => match store.wait_result(sid, &tid).await {
                Ok(Ok(value)) => JsonRpcResponse::success(id, value).into(),
                Ok(Err(err)) => JsonRpcResponse::error(id, err).into(),
                Err(e) => task_error_response(id, &e),
            },
        },
        _ => unreachable!("handle_tasks_method called with an unrouted method"),
    }
}

#[derive(Deserialize)]
struct RawTaskIdParams {
    #[serde(rename = "taskId")]
    task_id: String,
}

fn parse_task_id(params: Option<&Value>) -> Result<String, McpError> {
    let params = params.ok_or_else(|| McpError::invalid_params("missing `taskId`"))?;
    let raw: RawTaskIdParams = serde_json::from_value(params.clone())
        .map_err(|e| McpError::invalid_params(format!("invalid task params: {e}")))?;
    Ok(raw.task_id)
}

fn to_wire_status(s: TaskStatus) -> legacy::TaskStatus {
    match s {
        TaskStatus::Working => legacy::TaskStatus::Working,
        TaskStatus::Completed => legacy::TaskStatus::Completed,
        TaskStatus::Failed => legacy::TaskStatus::Failed,
        TaskStatus::Cancelled => legacy::TaskStatus::Cancelled,
    }
}

fn to_wire_task(s: &TaskSnapshot, poll_interval_ms: i64) -> legacy::Task {
    legacy::Task {
        created_at: s.created_at.clone(),
        last_updated_at: s.last_updated_at.clone(),
        poll_interval: Some(poll_interval_ms),
        status: to_wire_status(s.status),
        status_message: s.status_message.clone(),
        task_id: s.task_id.clone(),
        ttl: Some(s.ttl_ms),
    }
}

/// Spec error mapping (tasks.mdx §Error Handling): unknown ids and terminal
/// cancels are `-32602`; capacity exhaustion is an internal `-32603`.
fn task_error_response(id: RequestId, e: &TaskError) -> JsonRpcMessage {
    let (code, message) = match e {
        TaskError::NotFound => (
            -32602,
            "unknown task id (expired, evicted, or never created)",
        ),
        TaskError::AlreadyTerminal => (-32602, "task is already in a terminal status"),
        TaskError::CapacityExhausted => (-32603, "task capacity exhausted; retry later"),
    };
    JsonRpcResponse::error(
        id,
        JsonRpcError {
            code,
            message: message.to_owned(),
            data: None,
        },
    )
    .into()
}
