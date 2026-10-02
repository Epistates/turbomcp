//! Draft Tasks extension augmentation (SEP-2663): offering a `tools/call` to
//! call-augmenting extensions and preparing the underlying call as a
//! [`CallRunner`] the extension can spawn.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::Value;

use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcRequest, McpError, ProtocolVersion,
    RequestContext, RequestId,
};
use turbomcp_protocol::methods;
use turbomcp_protocol::v2026_07_28::types as v0728;

use crate::context::CallToolContext;
use crate::extension::{CallAugmentRequest, CallRunner, Extension};
use crate::mrtr::ClientHandle;
use crate::progress::ProgressReporter;
use crate::router::MethodRouter;
use crate::task_handle::{TaskHandle, TaskSlot};
use crate::traits::McpServerCore;

use super::params::parse_call_tool_params;
use super::{context_declares_extension, error_response};

// ---- draft Tasks extension augmentation (SEP-2663) -----------------------------

/// The Tasks extension rides the stateless wire, so its errors take that
/// revision's codes.
const VERSION: ProtocolVersion = ProtocolVersion::V2026_07_28;

fn wire_error(err: &McpError) -> JsonRpcError {
    err.to_jsonrpc_error(&VERSION)
}

/// Offer a draft `tools/call` to each call-augmenting extension the client
/// declared. The first extension to take over returns the response (a
/// `CreateTaskResult`); `None` means run the call normally. Only declared
/// clients are offered augmentation — SEP-2663 forbids returning a
/// `CreateTaskResult` to a client that didn't declare the extension.
pub(super) async fn try_augment_call<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    shared: &super::Shared,
    extensions: &[Arc<dyn Extension>],
    id: &RequestId,
) -> Option<JsonRpcMessage> {
    for ext in extensions {
        if !(ext.augments_calls() && context_declares_extension(ctx, ext.id())) {
            continue;
        }
        let (_, tool) = match super::capability::prepare_tool::<S, super::capability::DraftWire>(
            server,
            router,
            req,
            ctx,
            shared,
            id.clone(),
        )
        .await
        {
            Ok(prepared) => prepared,
            Err(response) => return Some(*response),
        };
        let run = match build_call_runner(
            server,
            router,
            req,
            ctx,
            (shared.validators.clone(), tool.output_schema.clone()),
        ) {
            Ok(run) => run,
            // A malformed `tools/call` envelope is `-32602` regardless of
            // augmentation (mirrors the normal `dispatch_capability` path).
            Err(e) => return Some(error_response(id.clone(), &e)),
        };
        if let Some(resp) = ext
            .augment_call(CallAugmentRequest {
                request: req.clone(),
                context: ctx.clone(),
                run,
                tool,
            })
            .await
        {
            return Some(resp);
        }
    }
    None
}

/// Prepare the underlying `tools/call` as a [`CallRunner`]: parse the envelope,
/// mint the task's cancellation token, wire it into a fresh context, and build
/// the handler future that renders to draft `CallToolResult` JSON (or the
/// JSON-RPC error). The originating request returns `CreateTaskResult`
/// immediately, so its stream is gone and there is no log channel; the
/// context reaches the task instead, through one late-bound slot:
/// `ctx.task`, `ctx.progress` (reports become the task's `statusMessage`,
/// the only progress channel a task has), and the task-mediated
/// `ClientHandle` (SEP-2663 in-execution `input_required` — published via
/// `inputRequests`, answered via `tasks/update`).
fn build_call_runner<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    contract: (Arc<crate::catalog::Validators>, Option<Value>),
) -> Result<CallRunner, McpError> {
    let params = parse_call_tool_params(req.params.as_ref())?;
    let cancel = CancellationToken::new();
    let mut call_ctx = ctx.clone();
    call_ctx.cancellation = cancel.clone();
    // The taskifying extension fills the slot via `CallRunner::attach_task`.
    // Capability gating of client input (SEP-2322 MUST) still applies — the
    // client's per-request declared capabilities travel with the handle.
    let slot = TaskSlot::default();
    let task = TaskHandle::bound(slot.clone());
    let handle = ClientHandle::task_mediated(ctx.client_capabilities.clone(), slot.clone());
    let fut = router.dispatch_call_tool(
        server.clone(),
        CallToolContext::new(call_ctx)
            .with_client(handle)
            .with_progress(ProgressReporter::for_task(task.clone()))
            .with_task(task),
        params,
    );
    let future: BoxFuture<'static, Result<Value, JsonRpcError>> = Box::pin(async move {
        match fut {
            None => Err(wire_error(&McpError::method_not_found(
                methods::request::TOOLS_CALL,
            ))),
            Some(f) => match f.await {
                Ok(result) => serde_json::to_value(v0728::CallToolResult::from(
                    contract
                        .0
                        .output(contract.1.as_ref(), result)
                        .map_err(|e| wire_error(&e))?,
                ))
                .map_err(|e| wire_error(&McpError::internal(e.to_string()))),
                Err(e) => Err(wire_error(&e)),
            },
        }
    });
    Ok(CallRunner::new(future, cancel).with_task_slot(slot))
}
