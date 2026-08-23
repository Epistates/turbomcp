//! Capability dispatch: the one generic path both protocol versions share.
//!
//! [`WireFamily`] selects the per-version result types; [`dispatch_capability`]
//! parses the request, builds the per-RPC context (client handle, progress,
//! logging), awaits the registered handler, and widens the neutral result to
//! the active wire. MRTR turn handling ([`mrtr_handle`]/[`finish_mrtr`],
//! SEP-2322) lives here because it is part of that dispatch contract.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use turbomcp_core::{
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpError, ProtocolVersion, RequestContext,
    RequestId, meta,
};
use turbomcp_protocol::v2025_06_18::types as v0618;
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_protocol::{methods, neutral};

use crate::context::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};
use crate::logging::LogSender;
use crate::mrtr::{ClientHandle, PendingRequests, StateSigner};
use crate::progress::ProgressReporter;
use crate::router::MethodRouter;
use crate::traits::McpServerCore;
use crate::visibility::{self, ComponentKind, VisibleComponent};

use super::params::{
    parse_call_tool_params, parse_complete_params, parse_get_prompt_params, parse_list_params,
    parse_read_resource_params,
};
use super::{Shared, connection_id, error_response_for, ok_value, session_id};

/// Fill the server's configured default cache policy (SEP-2549) into a
/// cacheable neutral result whose handler didn't set one. Applied on both wire
/// families — the legacy conversion has no cache fields and ignores the value.
fn with_cache_default<N>(
    fut: Option<BoxFuture<'static, Result<N, McpError>>>,
    policy: neutral::CachePolicy,
) -> Option<BoxFuture<'static, Result<N, McpError>>>
where
    N: neutral::Cacheable + Send + 'static,
{
    fut.map(|f| {
        async move {
            f.await.map(|mut n| {
                n.cache_policy_mut().get_or_insert(policy);
                n
            })
        }
        .boxed()
    })
}

/// Await a registered handler's future and widen its neutral result to the
/// active wire type `W`. A `None` future means the capability isn't registered
/// (e.g. `resources/read` on a tools-only server) → `method_not_found`.
async fn finish<N, W>(
    id: RequestId,
    method: &str,
    version: &ProtocolVersion,
    fut: Option<BoxFuture<'static, Result<N, McpError>>>,
) -> JsonRpcMessage
where
    W: Serialize + From<N>,
{
    match fut {
        None => error_response_for(id, version, &McpError::method_not_found(method)),
        Some(f) => match f.await {
            Ok(result) => ok_value(id, &W::from(result)),
            Err(e) => error_response_for(id, version, &e),
        },
    }
}

/// The per-version wire surface: one associated type per capability result.
/// Both versions dispatch through the same generic path; only the
/// `From<neutral>` target differs (the conversions live in
/// `turbomcp_protocol::neutral`).
pub(super) trait WireFamily {
    /// Whether this wire family delivers client interaction via MRTR
    /// (`InputRequiredResult`); the legacy family uses inline bidi instead.
    const MRTR: bool;
    /// A version of this family, for the error codes that are version-split
    /// (resource-not-found renumbered `-32002` -> `-32602` at the 2026-07-28
    /// RC). Any member of the family answers identically.
    const VERSION: ProtocolVersion;
    type ListTools: Serialize + From<neutral::ListToolsResult>;
    type CallTool: Serialize + From<neutral::CallToolResult>;
    type ListResources: Serialize + From<neutral::ListResourcesResult>;
    type ListResourceTemplates: Serialize + From<neutral::ListResourceTemplatesResult>;
    type ReadResource: Serialize + From<neutral::ReadResourceResult>;
    type ListPrompts: Serialize + From<neutral::ListPromptsResult>;
    type GetPrompt: Serialize + From<neutral::GetPromptResult>;
    type Complete: Serialize + From<neutral::CompleteResult>;
}

/// `2026-07-28` (modern, stateless).
pub(super) struct DraftWire;

impl WireFamily for DraftWire {
    const MRTR: bool = true;
    const VERSION: ProtocolVersion = ProtocolVersion::V2026_07_28;
    type ListTools = v0728::ListToolsResult;
    type CallTool = v0728::CallToolResult;
    type ListResources = v0728::ListResourcesResult;
    type ListResourceTemplates = v0728::ListResourceTemplatesResult;
    type ReadResource = v0728::ReadResourceResult;
    type ListPrompts = v0728::ListPromptsResult;
    type GetPrompt = v0728::GetPromptResult;
    type Complete = v0728::CompleteResult;
}

/// `2025-11-25` (legacy, stateful).
pub(super) struct LegacyWire;

impl WireFamily for LegacyWire {
    const MRTR: bool = false;
    const VERSION: ProtocolVersion = ProtocolVersion::V2025_11_25;
    type ListTools = legacy::ListToolsResult;
    type CallTool = legacy::CallToolResult;
    type ListResources = legacy::ListResourcesResult;
    type ListResourceTemplates = legacy::ListResourceTemplatesResult;
    type ReadResource = legacy::ReadResourceResult;
    type ListPrompts = legacy::ListPromptsResult;
    type GetPrompt = legacy::GetPromptResult;
    type Complete = legacy::CompleteResult;
}

/// `2025-06-18` (the previous stable revision, stateful).
///
/// Same dispatch path as [`LegacyWire`] — same methods, same session model,
/// same inline-bidi client interaction. Only the result types differ, and only
/// by the fields `2025-11-25` added (`icons`, task support); the conversions
/// step down from the `2025-11-25` wire, see
/// [`v2025_06_18::convert`](turbomcp_protocol::v2025_06_18::convert).
pub(super) struct Legacy0618Wire;

impl WireFamily for Legacy0618Wire {
    const MRTR: bool = false;
    const VERSION: ProtocolVersion = ProtocolVersion::V2025_06_18;
    type ListTools = v0618::ListToolsResult;
    type CallTool = v0618::CallToolResult;
    type ListResources = v0618::ListResourcesResult;
    type ListResourceTemplates = v0618::ListResourceTemplatesResult;
    type ReadResource = v0618::ReadResourceResult;
    type ListPrompts = v0618::ListPromptsResult;
    type GetPrompt = v0618::GetPromptResult;
    type Complete = v0618::CompleteResult;
}

/// Apply the installed visibility policy to a list result before it is widened
/// to the wire.
fn with_visibility<N>(
    fut: Option<BoxFuture<'static, Result<N, McpError>>>,
    shared: &Shared,
    ctx: &RequestContext,
    filter: fn(&visibility::Policy, &RequestContext, &mut N),
) -> Option<BoxFuture<'static, Result<N, McpError>>>
where
    N: Send + 'static,
{
    let policy = shared.visibility.clone();
    if policy.is_none() {
        return fut;
    }
    let ctx = ctx.clone();
    fut.map(|f| {
        async move {
            f.await.map(|mut n| {
                filter(&policy, &ctx, &mut n);
                n
            })
        }
        .boxed()
    })
}

/// What a call is addressing, for the pre-dispatch visibility check.
enum Component<'a> {
    Tool(&'a str),
    Resource(&'a str),
    Prompt(&'a str),
}

/// Whether the installed policy hides the component a call is addressing.
///
/// A policy decides on a component's *metadata*, but a `tools/call` carries
/// only a name — so this lists the corresponding capability to find it. That
/// extra list is the price of "hidden means unreachable"; it is paid only when
/// a policy is installed, and skipped entirely otherwise.
///
/// A component no list mentions is **not** treated as hidden: the handler owns
/// that answer, and it already produces the right unknown-tool /
/// unknown-prompt / not-found reply.
async fn hidden<S: McpServerCore>(
    shared: &Shared,
    router: &MethodRouter<S>,
    server: &S,
    ctx: &RequestContext,
    component: Component<'_>,
) -> bool {
    let Some(policy) = shared.visibility.as_ref() else {
        return false;
    };
    let params = neutral::ListParams::default();
    let judge = |kind, id: &str, meta: &Map<String, Value>| {
        !policy.is_visible(&VisibleComponent {
            kind,
            id,
            meta,
            request: ctx,
        })
    };
    match component {
        Component::Tool(name) => {
            let Some(fut) = router.dispatch_list_tools(
                server.clone(),
                ListToolsContext::new(ctx.clone()),
                params,
            ) else {
                return false;
            };
            let Ok(listed) = fut.await else { return false };
            listed
                .tools
                .iter()
                .find(|t| t.name == name)
                .is_some_and(|t| judge(ComponentKind::Tool, &t.name, &t.meta))
        }
        Component::Prompt(name) => {
            let Some(fut) = router.dispatch_list_prompts(
                server.clone(),
                ListPromptsContext::new(ctx.clone()),
                params,
            ) else {
                return false;
            };
            let Ok(listed) = fut.await else { return false };
            listed
                .prompts
                .iter()
                .find(|p| p.name == name)
                .is_some_and(|p| judge(ComponentKind::Prompt, &p.name, &p.meta))
        }
        Component::Resource(uri) => {
            if let Some(fut) = router.dispatch_list_resources(
                server.clone(),
                ListResourcesContext::new(ctx.clone()),
                params.clone(),
            ) && let Ok(listed) = fut.await
                && let Some(r) = listed.resources.iter().find(|r| r.uri == uri)
            {
                return judge(ComponentKind::Resource, &r.uri, &r.meta);
            }
            // Not a concrete resource — it may still be produced by a template,
            // which is where the policy's decision was recorded.
            let Some(fut) = router.dispatch_list_resource_templates(
                server.clone(),
                ListResourceTemplatesContext::new(ctx.clone()),
                params,
            ) else {
                return false;
            };
            let Ok(listed) = fut.await else { return false };
            listed
                .resource_templates
                .iter()
                .find(|t| {
                    crate::__macro_support::match_uri_template(&t.uri_template, uri).is_some()
                })
                .is_some_and(|t| judge(ComponentKind::ResourceTemplate, &t.uri_template, &t.meta))
        }
    }
}

pub(super) async fn dispatch_capability<S: McpServerCore, W: WireFamily>(
    server: S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    shared: &Shared,
    id: RequestId,
) -> JsonRpcMessage {
    let signer = &shared.signer;
    let pending = &shared.pending;
    let method = req.method.as_str();
    let ctx = ctx.clone();
    let list_params = parse_list_params(req.params.as_ref());
    match method {
        methods::request::TOOLS_LIST => {
            let fut =
                router.dispatch_list_tools(server, ListToolsContext::new(ctx.clone()), list_params);
            let fut = with_visibility(fut, shared, &ctx, visibility::filter_tools);
            let fut = with_cache_default(fut, shared.cache.tools_list);
            finish::<_, W::ListTools>(id, method, &W::VERSION, fut).await
        }
        methods::request::TOOLS_CALL => {
            let params = match parse_call_tool_params(req.params.as_ref()) {
                Ok(p) => p,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            // A hidden tool must be unreachable, not merely unlisted — and
            // refused exactly as an unknown one, or the refusal discloses what
            // the policy is hiding.
            if hidden(shared, router, &server, &ctx, Component::Tool(&params.name)).await {
                return ok_value(
                    id,
                    &W::CallTool::from(neutral::CallToolResult::error(format!(
                        "unknown tool: {}",
                        params.name
                    ))),
                );
            }
            let handle = match mrtr_handle::<W>(
                req,
                &ctx,
                signer,
                pending,
                shared.strict_elicitation_keys,
            ) {
                Ok(h) => h,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            let fut = router.dispatch_call_tool(
                server,
                CallToolContext::new(ctx.clone())
                    .with_client(handle.clone())
                    .with_progress(progress_reporter::<W>(req))
                    .with_log(log_sender::<W>(req, &ctx, router.has_logging())),
                params,
            );
            let subject = ctx.identity.subject().map(str::to_owned);
            finish_mrtr::<_, W::CallTool>(
                id,
                MrtrTurn {
                    method,
                    version: &W::VERSION,
                    subject,
                    handle: &handle,
                    signer,
                    mrtr_enabled: W::MRTR,
                },
                fut,
            )
            .await
        }
        methods::request::RESOURCES_LIST => {
            let fut = router.dispatch_list_resources(
                server,
                ListResourcesContext::new(ctx.clone()),
                list_params,
            );
            let fut = with_visibility(fut, shared, &ctx, visibility::filter_resources);
            let fut = with_cache_default(fut, shared.cache.resources_list);
            finish::<_, W::ListResources>(id, method, &W::VERSION, fut).await
        }
        methods::request::RESOURCES_TEMPLATES_LIST => {
            let fut = router.dispatch_list_resource_templates(
                server,
                ListResourceTemplatesContext::new(ctx.clone()),
                list_params,
            );
            let fut = with_visibility(fut, shared, &ctx, visibility::filter_resource_templates);
            let fut = with_cache_default(fut, shared.cache.resource_templates_list);
            finish::<_, W::ListResourceTemplates>(id, method, &W::VERSION, fut).await
        }
        methods::request::RESOURCES_READ => {
            let params = match parse_read_resource_params(req.params.as_ref()) {
                Ok(p) => p,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            if hidden(
                shared,
                router,
                &server,
                &ctx,
                Component::Resource(&params.uri),
            )
            .await
            {
                return error_response_for(
                    id,
                    &W::VERSION,
                    &McpError::resource_not_found(params.uri),
                );
            }
            let handle = match mrtr_handle::<W>(
                req,
                &ctx,
                signer,
                pending,
                shared.strict_elicitation_keys,
            ) {
                Ok(h) => h,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            let fut = router.dispatch_read_resource(
                server,
                ReadResourceContext::new(ctx.clone())
                    .with_client(handle.clone())
                    .with_progress(progress_reporter::<W>(req))
                    .with_log(log_sender::<W>(req, &ctx, router.has_logging())),
                params,
            );
            let fut = with_cache_default(fut, shared.cache.resources_read);
            let subject = ctx.identity.subject().map(str::to_owned);
            finish_mrtr::<_, W::ReadResource>(
                id,
                MrtrTurn {
                    method,
                    version: &W::VERSION,
                    subject,
                    handle: &handle,
                    signer,
                    mrtr_enabled: W::MRTR,
                },
                fut,
            )
            .await
        }
        methods::request::PROMPTS_LIST => {
            let fut = router.dispatch_list_prompts(
                server,
                ListPromptsContext::new(ctx.clone()),
                list_params,
            );
            let fut = with_visibility(fut, shared, &ctx, visibility::filter_prompts);
            let fut = with_cache_default(fut, shared.cache.prompts_list);
            finish::<_, W::ListPrompts>(id, method, &W::VERSION, fut).await
        }
        methods::request::PROMPTS_GET => {
            let params = match parse_get_prompt_params(req.params.as_ref()) {
                Ok(p) => p,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            if hidden(
                shared,
                router,
                &server,
                &ctx,
                Component::Prompt(&params.name),
            )
            .await
            {
                return error_response_for(
                    id,
                    &W::VERSION,
                    &McpError::invalid_params(format!("unknown prompt: {}", params.name)),
                );
            }
            let handle = match mrtr_handle::<W>(
                req,
                &ctx,
                signer,
                pending,
                shared.strict_elicitation_keys,
            ) {
                Ok(h) => h,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            let fut = router.dispatch_get_prompt(
                server,
                GetPromptContext::new(ctx.clone())
                    .with_client(handle.clone())
                    .with_progress(progress_reporter::<W>(req))
                    .with_log(log_sender::<W>(req, &ctx, router.has_logging())),
                params,
            );
            let subject = ctx.identity.subject().map(str::to_owned);
            finish_mrtr::<_, W::GetPrompt>(
                id,
                MrtrTurn {
                    method,
                    version: &W::VERSION,
                    subject,
                    handle: &handle,
                    signer,
                    mrtr_enabled: W::MRTR,
                },
                fut,
            )
            .await
        }
        methods::request::COMPLETION_COMPLETE => {
            let params = match parse_complete_params(req.params.as_ref()) {
                Ok(p) => p,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            };
            let fut = router.dispatch_complete(server, CompleteContext::new(ctx), params);
            finish::<_, W::Complete>(id, method, &W::VERSION, fut).await
        }
        _ => unreachable!("dispatch_capability called with an unrouted method"),
    }
}

// ---- MRTR (SEP-2322) -----------------------------------------------------------

#[derive(Deserialize, Default)]
struct RawMrtrFields {
    #[serde(rename = "inputResponses", default)]
    input_responses: Option<BTreeMap<String, Value>>,
    #[serde(rename = "requestState", default)]
    request_state: Option<String>,
}

/// Build the request's [`ClientHandle`]: on the draft, an MRTR coordinator
/// seeded with the retry's `inputResponses` and verified `requestState`
/// (verification failure rejects the request before the handler runs — the
/// blob is attacker-controlled); on the legacy family, an inline-bidi handle
/// bound to the request's session.
fn mrtr_handle<W: WireFamily>(
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    signer: &StateSigner,
    pending: &Arc<PendingRequests>,
    strict_keys: bool,
) -> Result<ClientHandle, McpError> {
    if !W::MRTR {
        // The legacy session gate ran before dispatch, so the session id is
        // present on this path; its absence means no client channel.
        return Ok(match session_id(req.params.as_ref()) {
            Some(session) => ClientHandle::bidi(
                session,
                connection_id(req.params.as_ref()).unwrap_or_default(),
                Arc::clone(pending),
                ctx.client_capabilities.clone(),
            ),
            None => ClientHandle::unavailable("no session for inline bidirectional requests"),
        });
    }
    let fields: RawMrtrFields = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .unwrap_or_default();
    let state_in = match &fields.request_state {
        Some(token) => Some(signer.verify(&req.method, ctx.identity.subject(), token)?),
        None => None,
    };
    Ok(ClientHandle::mrtr(
        connection_id(req.params.as_ref()).unwrap_or_default(),
        ctx.client_capabilities.clone(),
        fields.input_responses.unwrap_or_default(),
        state_in,
        strict_keys,
    ))
}

/// Everything [`finish_mrtr`] needs about the *request* it is completing, as
/// one value (the alternative trips clippy's argument limit).
pub(super) struct MrtrTurn<'a> {
    /// The originating method — names the error and binds the signed state.
    pub(super) method: &'a str,
    /// The wire family's version, for the version-split error codes.
    pub(super) version: &'a ProtocolVersion,
    /// The authenticated principal, bound into any minted `requestState`.
    pub(super) subject: Option<String>,
    /// The handler's client channel: what it recorded, what it stashed.
    pub(super) handle: &'a ClientHandle,
    pub(super) signer: &'a StateSigner,
    /// Whether this wire answers `InputRequiredResult` at all (legacy uses
    /// inline bidi, so a sentinel there is a leak, not a turn).
    pub(super) mrtr_enabled: bool,
}

/// [`finish`], plus MRTR-abort interception: when the handler bailed with the
/// [`McpError::InputRequired`] sentinel on an MRTR-capable wire, answer an
/// `InputRequiredResult` carrying the recorded input requests and the signed
/// outbound `requestState` (the spec's MUST: at least one of the two).
async fn finish_mrtr<N, WIRE>(
    id: RequestId,
    turn: MrtrTurn<'_>,
    fut: Option<BoxFuture<'static, Result<N, McpError>>>,
) -> JsonRpcMessage
where
    WIRE: Serialize + From<N>,
{
    let MrtrTurn {
        method,
        version,
        subject,
        handle,
        signer,
        mrtr_enabled,
    } = turn;
    let Some(f) = fut else {
        return error_response_for(id, version, &McpError::method_not_found(method));
    };
    match f.await {
        Ok(result) => ok_value(id, &WIRE::from(result)),
        Err(McpError::InputRequired) if mrtr_enabled => {
            let collected = handle.collected();
            let state_out = handle.state_out();
            if collected.is_empty() && state_out.is_none() {
                // The spec requires at least one of inputRequests/requestState;
                // a bare sentinel means a handler leaked it manually.
                return error_response_for(
                    id,
                    version,
                    &McpError::internal("MRTR abort recorded no input requests"),
                );
            }
            let mut result = Map::new();
            result.insert(
                "resultType".to_owned(),
                serde_json::json!(neutral::result_type::INPUT_REQUIRED),
            );
            if !collected.is_empty() {
                result.insert(
                    "inputRequests".to_owned(),
                    Value::Object(collected.into_iter().collect()),
                );
            }
            if let Some(data) = state_out {
                match signer.sign(method, subject.as_deref(), &data) {
                    Ok(token) => {
                        result.insert("requestState".to_owned(), serde_json::json!(token));
                    }
                    Err(e) => return error_response_for(id, version, &e),
                }
            }
            JsonRpcResponse::success(id, Value::Object(result)).into()
        }
        Err(e) => error_response_for(id, version, &e),
    }
}

/// Build the request's [`LogSender`]: live when the server enabled `logging`
/// AND the client opted in (the context's `log_level` carries the opt-in from
/// either the draft `_meta` key or the legacy session's `setLevel`). Routing
/// mirrors [`progress_reporter`].
fn log_sender<W: WireFamily>(
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    logging_enabled: bool,
) -> LogSender {
    let Some(min) = ctx.log_level.filter(|_| logging_enabled) else {
        return LogSender::disabled();
    };
    let connection = connection_id(req.params.as_ref())
        .unwrap_or_default()
        .to_owned();
    let session = if W::MRTR {
        String::new()
    } else {
        session_id(req.params.as_ref())
            .unwrap_or_default()
            .to_owned()
    };
    LogSender::new(min, connection, session)
}

/// Build the request's [`ProgressReporter`]: live when the request carried a
/// `_meta.progressToken` (string or integer per the progress spec — anything
/// else is treated as absent, with a warning), inert otherwise. Notifications
/// route to the request's own stream; the legacy family may fall back to the
/// session `GET` stream, the draft never does.
fn progress_reporter<W: WireFamily>(req: &JsonRpcRequest) -> ProgressReporter {
    let token = req
        .params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(meta::keys::PROGRESS_TOKEN));
    let Some(token) = token else {
        return ProgressReporter::disabled();
    };
    if !(token.is_string() || token.is_i64() || token.is_u64()) {
        tracing::warn!(?token, "progressToken must be a string or integer; ignored");
        return ProgressReporter::disabled();
    }
    let connection = connection_id(req.params.as_ref())
        .unwrap_or_default()
        .to_owned();
    let session = if W::MRTR {
        String::new()
    } else {
        session_id(req.params.as_ref())
            .unwrap_or_default()
            .to_owned()
    };
    ProgressReporter::new(token.clone(), connection, session)
}
