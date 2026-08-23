//! Draft Tasks extension augmentation (SEP-2663): offering a `tools/call` to
//! call-augmenting extensions and preparing the underlying call as a
//! [`CallRunner`] the extension can spawn.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::Value;

use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcRequest, McpError, RequestContext,
    RequestId,
};
use turbomcp_protocol::methods;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_service::mcp_to_jsonrpc_error;

use crate::context::CallToolContext;
use crate::extension::{CallAugmentRequest, CallRunner, Extension};
use crate::mrtr::ClientHandle;
use crate::router::MethodRouter;
use crate::traits::McpServerCore;

use super::params::parse_call_tool_params;
use super::{connection_id, context_declares_extension, error_response};

// ---- draft Tasks extension augmentation (SEP-2663) -----------------------------

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
    extensions: &[Arc<dyn Extension>],
    id: &RequestId,
) -> Option<JsonRpcMessage> {
    for ext in extensions {
        if !(ext.augments_calls() && context_declares_extension(ctx, ext.id())) {
            continue;
        }
        let run = match build_call_runner(server, router, req, ctx) {
            Ok(run) => run,
            // A malformed `tools/call` envelope is `-32602` regardless of
            // augmentation (mirrors the normal `dispatch_capability` path).
            Err(e) => return Some(error_response(id.clone(), &e)),
        };
        let connection_id = connection_id(req.params.as_ref()).map(str::to_owned);
        if let Some(resp) = ext
            .augment_call(CallAugmentRequest {
                request: req.clone(),
                context: ctx.clone(),
                connection_id,
                run,
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
/// JSON-RPC error). Taskified calls get no progress/log channel — the
/// originating request returns `CreateTaskResult` immediately, so its stream is
/// gone. Client input DOES work mid-task: the context's `ClientHandle` is
/// task-mediated (SEP-2663 in-execution `input_required` — published via
/// `inputRequests`, answered via `tasks/update`).
fn build_call_runner<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: &RequestContext,
) -> Result<CallRunner, McpError> {
    let params = parse_call_tool_params(req.params.as_ref())?;
    let cancel = CancellationToken::new();
    let mut call_ctx = ctx.clone();
    call_ctx.cancellation = cancel.clone();
    // Mid-task client input (SEP-2663 in-execution `input_required`): the
    // handle publishes input requests through the late-bound broker slot the
    // taskifying extension attaches via `CallRunner::attach_input_broker`.
    // Capability gating (SEP-2322 MUST) still applies — the client's
    // per-request declared capabilities travel with the handle.
    let input_slot = crate::extension::TaskInputSlot::default();
    let handle = ClientHandle::task_mediated(ctx.client_capabilities.clone(), input_slot.clone());
    let fut = router.dispatch_call_tool(
        server.clone(),
        CallToolContext::new(call_ctx).with_client(handle),
        params,
    );
    let future: BoxFuture<'static, Result<Value, JsonRpcError>> = Box::pin(async move {
        match fut {
            None => Err(mcp_to_jsonrpc_error(&McpError::method_not_found(
                methods::request::TOOLS_CALL,
            ))),
            Some(f) => match f.await {
                Ok(result) => serde_json::to_value(v0728::CallToolResult::from(result))
                    .map_err(|e| mcp_to_jsonrpc_error(&McpError::internal(e.to_string()))),
                Err(e) => Err(mcp_to_jsonrpc_error(&e)),
            },
        }
    });
    Ok(CallRunner::new(future, cancel).with_input_slot(input_slot))
}
