//! Core Tasks (`2025-11-25`): task-augmented `tools/call`, the `tasks/*`
//! methods, and per-tool `taskSupport` advertisement. The draft serves Tasks
//! as an extension instead (see [`super::augment`]).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};

use turbomcp_core::meta::keys::RELATED_TASK;
use turbomcp_core::{
    CancellationToken, Extensions, JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
    McpError, McpResult, ProtocolVersion, RequestContext, RequestId,
};
use turbomcp_protocol::methods;
use turbomcp_protocol::neutral::{self, TaskSupport};
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_service::Peer;
use turbomcp_service::catch_panic;

use crate::context::{CallToolContext, ListToolsContext};
use crate::mrtr::ClientHandle;
use crate::router::MethodRouter;
use crate::subscriptions::Route;
use crate::task_handle::{TaskHandle, TaskLink, TaskSlot};
use crate::tasks::{
    NewTask, TaskBackend, TaskError, TaskOutcome, TaskOwner, TaskSnapshot, TaskStatus,
};
use crate::traits::McpServerCore;

use super::task_input::TaskInputRelay;
use super::{error_response_for, ok_value, session_id};

/// Core Tasks exist only on `2025-11-25`, so this module speaks its codes.
const VERSION: ProtocolVersion = ProtocolVersion::V2025_11_25;

fn error_response(id: RequestId, err: &McpError) -> JsonRpcMessage {
    error_response_for(id, &VERSION, err)
}

// ---- core Tasks (2025-11-25) ---------------------------------------------------

/// How many tasks one `tasks/list` page carries.
const TASKS_PAGE_SIZE: usize = 50;

/// The TTL a task gets when the client doesn't ask for one, in milliseconds.
const DEFAULT_TTL_MS: i64 = 300_000; // 5 minutes
/// The most TTL a client can ask for ("Receivers MAY override the requested
/// `ttl` duration"), in milliseconds.
const MAX_TTL_MS: i64 = 3_600_000; // 1 hour

/// A session's tasks are its own.
pub(super) fn owner(sid: &str) -> TaskOwner {
    TaskOwner::Session(sid.to_owned())
}

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
/// `CreateTaskResult` (spec §Creating Tasks). The handler's `ctx.client`
/// asks through the task, and `relay` carries its requests to the client
/// (tasks.mdx §Input Required Status).
#[allow(clippy::too_many_arguments)]
pub(super) async fn task_augmented_call<S: McpServerCore>(
    server: S,
    router: &MethodRouter<S>,
    (store, relay): (&Arc<dyn TaskBackend>, &Arc<TaskInputRelay>),
    ctx: RequestContext,
    req: &JsonRpcRequest,
    id: RequestId,
    params: neutral::CallToolParams,
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
    // A TTL of zero or less asks for a task that is gone before anyone could
    // poll it; clamping it to zero used to do exactly that, silently.
    if task_meta.ttl.is_some_and(|ttl| ttl <= 0) {
        return error_response(
            id,
            &McpError::invalid_params("a task `ttl` must be a positive number of milliseconds"),
        );
    }
    // The task's token doubles as the handler's request cancellation, so
    // `tasks/cancel` (and ttl purge) reach a cooperative handler.
    let token = CancellationToken::new();
    // The legacy gate guarantees a session id by the time we're here.
    let sid = session_id(&ctx.extensions).unwrap_or_default().to_owned();
    let mut ctx = ctx;
    ctx.cancellation = token.clone();
    // Filled once the task exists, before the work starts. "The
    // `progressToken` provided in the initial request remains valid
    // throughout the task lifetime", so progress both notifies and becomes
    // the task's status message.
    let slot = TaskSlot::default();
    let task = TaskHandle::bound(slot.clone());
    let progress = super::capability::progress_reporter::<super::capability::LegacyWire>(req, &ctx)
        .with_task(task.clone());
    // The originating request's stream is done once `CreateTaskResult` goes
    // out (on HTTP), so input requests take the relay's streams instead.
    let route = Route::for_request(&ctx.extensions, true);
    let client =
        ClientHandle::task_mediated(ctx.client_capabilities.clone(), slot.clone(), VERSION);
    let call_ctx = CallToolContext::new(ctx)
        .with_client(client)
        .with_progress(progress)
        .with_task(task);
    let Some(fut) = router.dispatch_call_tool(server, call_ctx, params) else {
        return error_response(
            id,
            &McpError::method_not_found(methods::request::TOOLS_CALL),
        );
    };

    let fut = async move { contract.0.output(contract.1.as_ref(), fut.await?) };

    let ttl_ms = task_meta.ttl.unwrap_or(DEFAULT_TTL_MS).min(MAX_TTL_MS);
    let snap = match store
        .create(&owner(&sid), NewTask::new(Some(ttl_ms)), token.clone())
        .await
    {
        Ok(s) => s,
        Err(e) => return task_error_response(id, &e),
    };
    let tracked = relay.track(&snap.task_id, Arc::clone(store), owner(&sid), route);
    let hook_relay = Arc::clone(relay);
    let _ = slot.set(
        TaskLink::new(Arc::clone(store), snap.task_id.clone(), token.clone()).on_change(
            move |task_id| {
                let relay = Arc::clone(&hook_relay);
                async move { relay.deliver(&task_id).await }
            },
        ),
    );

    let store = Arc::clone(store);
    let task_id = snap.task_id.clone();
    let span = tracing::info_span!("mcp.task", "mcp.task.id" = %task_id);
    let work = async move {
        let _tracked = tracked;
        tokio::select! {
            () = token.cancelled() => {
                // `tasks/cancel` (or expiry purge) already transitioned the
                // record; dropping `fut` aborts the handler.
            }
            // A panic would otherwise unwind this task with the record still
            // `working`, and `tasks/result` would block until the TTL: every
            // other path answers a panicking handler `-32603`, and so does this.
            out = catch_panic(fut) => {
                let outcome = match out {
                    Ok(Ok(result)) => {
                        let failed = result.is_error;
                        match serde_json::to_value(legacy::CallToolResult::from(result)) {
                            // "when the tool result has `isError` set to `true`,
                            // the task should reach `failed` status."
                            Ok(v) if failed => TaskOutcome::FailedResult {
                                result: v,
                                message: Some("the tool reported an error".to_owned()),
                            },
                            Ok(v) => TaskOutcome::Completed(v),
                            Err(e) => TaskOutcome::Error(JsonRpcError {
                                code: turbomcp_core::codes::INTERNAL_ERROR,
                                message: format!("serialize result: {e}"),
                                data: None,
                            }),
                        }
                    }
                    Ok(Err(e)) => TaskOutcome::Error(e.to_jsonrpc_error(&VERSION)),
                    Err(panic) => {
                        tracing::error!(panic, task = %task_id, "task handler panicked");
                        TaskOutcome::Error(JsonRpcError {
                            code: turbomcp_core::codes::INTERNAL_ERROR,
                            message: "handler panicked".to_owned(),
                            data: None,
                        })
                    }
                };
                store.complete(&task_id, outcome).await;
            }
        }
    };
    // The work runs in a span of its own, parented to the call that
    // created it: the call's span ends as soon as the task is created,
    // and the work used to run outside any span at all.
    tokio::spawn(tracing::Instrument::instrument(work, span));

    ok_value(
        id,
        &legacy::CreateTaskResult {
            meta: Map::new(),
            task: to_wire_task(&snap),
        },
    )
}

/// Whether any tool in the catalogue declares its own task support.
///
/// That decides what an undeclared tool gets once Tasks are on: when some tool
/// opts in individually (`#[tool(task)]`) the rest are `forbidden`, and when
/// none does, Tasks were switched on for every tool and each is `optional`.
/// Asked of the whole catalogue, not one page, so every page of `tools/list`
/// and the `tools/call` gate agree.
pub(super) async fn any_tool_declares_task_support<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    ctx: &RequestContext,
) -> McpResult<bool> {
    let found = crate::catalog::find(
        |params| {
            let listed = router.dispatch_list_tools(
                server.clone(),
                ListToolsContext::new(ctx.clone()),
                params,
            );
            async move {
                let page = listed
                    .ok_or_else(|| McpError::method_not_found(methods::request::TOOLS_LIST))?
                    .await?;
                Ok((page.tools, page.next_cursor))
            }
        },
        |tool: &neutral::Tool| tool.task_support.is_some(),
    )
    .await?;
    Ok(found.is_some())
}

/// The task support `tool` actually has on a server with Tasks enabled.
pub(super) fn effective_task_support(tool: &neutral::Tool, any_declared: bool) -> TaskSupport {
    tool.task_support.unwrap_or(if any_declared {
        TaskSupport::Forbidden
    } else {
        TaskSupport::Optional
    })
}

/// Spell out every tool's effective task support. The conversion layer can't
/// know Tasks are on, so `tools/list` applies this to the neutral result —
/// after the visibility filter, on the same path every other list takes.
pub(super) fn with_task_support(
    mut result: neutral::ListToolsResult,
    any_declared: bool,
) -> neutral::ListToolsResult {
    for tool in &mut result.tools {
        tool.task_support = Some(effective_task_support(tool, any_declared));
    }
    result
}

pub(super) async fn handle_tasks_method(
    (store, relay): (&Arc<dyn TaskBackend>, &Arc<TaskInputRelay>),
    sid: &str,
    method: &str,
    req: &JsonRpcRequest,
    ext: &Extensions,
    id: RequestId,
) -> JsonRpcMessage {
    match method {
        methods::request::TASKS_LIST => {
            let cursor = req
                .params
                .as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(Value::as_str);
            match store.list(&owner(sid), cursor, TASKS_PAGE_SIZE).await {
                Ok((page, next_cursor)) => ok_value(
                    id,
                    &legacy::ListTasksResult {
                        meta: Map::new(),
                        next_cursor,
                        tasks: page.iter().map(to_wire_task).collect(),
                    },
                ),
                Err(e) => task_error_response(id, &e),
            }
        }
        methods::request::TASKS_GET => match parse_task_id(req.params.as_ref()) {
            Err(e) => error_response(id, &e),
            Ok(tid) => match store.get(&owner(sid), &tid).await {
                Ok(s) => ok_value(
                    id,
                    &legacy::GetTaskResult {
                        created_at: s.created_at.clone(),
                        last_updated_at: s.last_updated_at.clone(),
                        meta: Map::new(),
                        poll_interval: s.poll_interval_ms,
                        status: to_wire_status(s.status),
                        status_message: s.status_message.clone(),
                        task_id: s.task_id.clone(),
                        ttl: s.ttl_ms,
                        extra: Map::new(),
                    },
                ),
                Err(e) => task_error_response(id, &e),
            },
        },
        methods::request::TASKS_CANCEL => match parse_task_id(req.params.as_ref()) {
            Err(e) => error_response(id, &e),
            Ok(tid) => match store.cancel(&owner(sid), &tid).await {
                Ok(s) => ok_value(
                    id,
                    &legacy::CancelTaskResult {
                        created_at: s.created_at.clone(),
                        last_updated_at: s.last_updated_at.clone(),
                        meta: Map::new(),
                        poll_interval: s.poll_interval_ms,
                        status: to_wire_status(s.status),
                        status_message: s.status_message.clone(),
                        task_id: s.task_id.clone(),
                        ttl: s.ttl_ms,
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
            Ok(tid) => match await_result((store, relay), &owner(sid), &tid, ext).await {
                // "The `tasks/result` operation MUST include this metadata in
                // its response, as the result structure itself does not contain
                // the task ID."
                Ok(Ok(value)) => {
                    JsonRpcResponse::success(id, with_related_task(value, &tid)).into()
                }
                Ok(Err(err)) => JsonRpcResponse::error(id, err).into(),
                Err(e) => task_error_response(id, &e),
            },
        },
        _ => unreachable!("handle_tasks_method called with an unrouted method"),
    }
}

/// `tasks/result`'s wait for the outcome. Meanwhile the call's stream is
/// where the task's input requests go: "When the requestor encounters the
/// `input_required` status, it SHOULD preemptively call `tasks/result`".
async fn await_result(
    (store, relay): (&Arc<dyn TaskBackend>, &Arc<TaskInputRelay>),
    owner: &TaskOwner,
    task_id: &str,
    ext: &Extensions,
) -> Result<Result<Value, JsonRpcError>, TaskError> {
    let stream = ext
        .get::<Peer>()
        .and_then(|peer| relay.result_stream(task_id, owner, peer.clone()));
    if stream.is_some() {
        relay.deliver(task_id).await;
    }
    let outcome = store.wait_result(owner, task_id).await;
    drop(stream);
    outcome
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

/// `value` with `_meta["io.modelcontextprotocol/related-task"]` naming `task_id`,
/// keeping any `_meta` it already carries.
pub(super) fn with_related_task(mut value: Value, task_id: &str) -> Value {
    if let Some(result) = value.as_object_mut() {
        let meta = result
            .entry("_meta")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(meta) = meta.as_object_mut() {
            meta.insert(
                RELATED_TASK.to_owned(),
                serde_json::json!({ "taskId": task_id }),
            );
        }
    }
    value
}

fn to_wire_status(s: TaskStatus) -> legacy::TaskStatus {
    match s {
        TaskStatus::Working => legacy::TaskStatus::Working,
        TaskStatus::InputRequired => legacy::TaskStatus::InputRequired,
        TaskStatus::Completed => legacy::TaskStatus::Completed,
        TaskStatus::Failed => legacy::TaskStatus::Failed,
        TaskStatus::Cancelled => legacy::TaskStatus::Cancelled,
    }
}

fn to_wire_task(s: &TaskSnapshot) -> legacy::Task {
    legacy::Task {
        created_at: s.created_at.clone(),
        last_updated_at: s.last_updated_at.clone(),
        poll_interval: s.poll_interval_ms,
        status: to_wire_status(s.status),
        status_message: s.status_message.clone(),
        task_id: s.task_id.clone(),
        ttl: s.ttl_ms,
    }
}

/// Spec error mapping (tasks.mdx §Error Handling): unknown ids and terminal
/// cancels are `-32602`; capacity exhaustion is an internal `-32603`.
fn task_error_response(id: RequestId, e: &TaskError) -> JsonRpcMessage {
    let (code, message) = match e {
        TaskError::NotFound => (
            turbomcp_core::codes::INVALID_PARAMS,
            "unknown task id (expired, evicted, or never created)",
        ),
        TaskError::AlreadyTerminal => (
            turbomcp_core::codes::INVALID_PARAMS,
            "task is already in a terminal status",
        ),
        TaskError::CapacityExhausted => (
            turbomcp_core::codes::INTERNAL_ERROR,
            "task capacity exhausted; retry later",
        ),
        TaskError::OwnerLimitReached => (
            turbomcp_core::codes::INTERNAL_ERROR,
            "this session has as many tasks running as it may; retry when one finishes",
        ),
        TaskError::InvalidCursor => (turbomcp_core::codes::INVALID_PARAMS, "invalid cursor"),
        TaskError::Unavailable(why) => {
            tracing::warn!(error = %why, "the task backend failed");
            (
                turbomcp_core::codes::INTERNAL_ERROR,
                "the task store is unavailable; retry later",
            )
        }
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
