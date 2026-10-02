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
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpError, McpResult, ObservedHeaders,
    ProtocolVersion, RequestContext, RequestId, SessionId, meta,
};
use turbomcp_protocol::v2025_06_18::types as v0618;
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_protocol::{methods, neutral};
use turbomcp_service::mcp_headers;

use crate::context::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};
use crate::logging::LogSender;
use crate::mrtr::{ClientHandle, PendingRequests, StateSealer};
use crate::progress::ProgressReporter;
use crate::router::MethodRouter;
use crate::traits::McpServerCore;
use crate::visibility::{self, ComponentKind, VisibleComponent};

use super::params::{
    parse_call_tool_params, parse_complete_params, parse_get_prompt_params, parse_list_params,
    parse_read_resource_params,
};
use super::{Shared, argument_at, collect_header_params, error_response_for, ok_value};
use crate::subscriptions::Route;

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
    Resource(&'a str),
    Prompt(&'a str),
}

/// Enforce SEP-2243's mirror rules for one `tools/call`: for every
/// `x-mcp-header` argument the tool declares, a value in `arguments` must
/// arrive with its `Mcp-Param-*` header, the header must decode to that value,
/// and a header with no value behind it is a mismatch too. "Any server that
/// processes the message body MUST validate that encoded header values, after
/// decoding if Base64-encoded, match the corresponding values in the request
/// body."
///
/// This is the one place that knows the schema, so it is the one place that
/// can check: the annotation may name a nested property, or one whose header
/// name differs from the property name. (The transport used to match headers
/// to top-level arguments by name, which waved a mismatched nested value
/// through and refused a correctly mirrored renamed one.) Headers the tool
/// doesn't declare are ignored, per the forwarding rule for intermediaries.
///
/// Skipped entirely unless the transport reported the mirrors it saw: only
/// Streamable HTTP has headers, and elsewhere the annotation is inert (the
/// spec lets non-HTTP transports ignore it).
fn check_header_mirrors(
    observed: Option<&ObservedHeaders>,
    params: &neutral::CallToolParams,
    tool: &neutral::Tool,
) -> McpResult<()> {
    let Some(observed) = observed else {
        return Ok(());
    };
    let mut declared = Vec::new();
    collect_header_params(&tool.input_schema, &mut Vec::new(), &mut declared);
    let arguments = Value::Object(params.arguments.clone());
    for param in declared {
        let header = &param.header;
        let mismatch = |why: &str| {
            Err(McpError::HeaderMismatch(format!(
                "Mcp-Param-{header} {why}"
            )))
        };
        match (argument_at(&arguments, &param.path), observed.get(header)) {
            (None, None) => {}
            (Some(_), None) => return mismatch("header is missing"),
            (None, Some(_)) => return mismatch("header has no matching value in the request body"),
            (Some(_), Some(None)) => return mismatch("header contains invalid characters"),
            (Some(body), Some(Some(raw))) => {
                let Some(decoded) = mcp_headers::decode_value(raw) else {
                    return mismatch("header has a malformed Base64 sentinel");
                };
                if !mirrors(body, &decoded) {
                    return mismatch("header does not match the request body");
                }
            }
        }
    }
    Ok(())
}

/// Whether a decoded header value mirrors `body`. Integers compare
/// numerically: "servers SHOULD compare the header value and the body value
/// numerically rather than as strings (e.g., `42.0` and `42` are considered
/// equal)", so a Python client's `42.0` isn't a spurious mismatch.
fn mirrors(body: &Value, header: &str) -> bool {
    match body {
        Value::Number(n) => {
            const JS_SAFE: f64 = 9_007_199_254_740_991.0;
            let (Some(body), Ok(header)) = (n.as_f64(), header.trim().parse::<f64>()) else {
                return false;
            };
            body.fract() == 0.0 && body.abs() <= JS_SAFE && body == header
        }
        other => mcp_headers::render_argument(other).is_some_and(|rendered| rendered == header),
    }
}

/// [`hidden`] for a resource URI, for the paths that address one without going
/// through [`dispatch_capability`] — the legacy `resources/subscribe` arm and
/// the draft `subscriptions/listen` filter.
///
/// Both deliver `notifications/resources/updated`, so a policy that stops a
/// caller *reading* a resource has to stop them watching it change: the update
/// notification names the URI, and a stream of them is a side channel on a
/// resource the caller was refused.
pub(super) async fn resource_hidden<S: McpServerCore>(
    shared: &Shared,
    router: &MethodRouter<S>,
    server: &S,
    ctx: &RequestContext,
    uri: &str,
) -> McpResult<bool> {
    hidden(shared, router, server, ctx, Component::Resource(uri)).await
}

/// Whether the installed policy hides the component a call is addressing.
///
/// A policy decides on a component's *metadata*, but a `tools/call` carries
/// only a name — so this lists the corresponding capability to find it. That
/// extra list is the price of "hidden means unreachable"; it is paid only when
/// a policy is installed, and skipped entirely otherwise.
///
/// A component no lookup finds **is** treated as hidden: the policy can only
/// judge what it can see, so what it can't see is refused, exactly as one it
/// hides. Deny by default, as every peer SDK with a component registry does.
/// A server that serves URIs it doesn't list (legal: the spec never requires
/// a readable URI to appear in `resources/list`) makes them judgeable by
/// overriding [`WithResources::lookup_resource`](crate::WithResources::lookup_resource)
/// to return a `Resource` for them, with the metadata the policy decides on.
async fn hidden<S: McpServerCore>(
    shared: &Shared,
    router: &MethodRouter<S>,
    server: &S,
    ctx: &RequestContext,
    component: Component<'_>,
) -> McpResult<bool> {
    if shared.visibility.is_none() {
        return Ok(false);
    }
    let judge =
        |kind, id: &str, meta: &Map<String, Value>| policy_hides(shared, ctx, kind, id, meta);
    match component {
        Component::Prompt(name) => {
            let Some(fut) = router.dispatch_lookup_prompt(
                server.clone(),
                ListPromptsContext::new(ctx.clone()),
                name.into(),
            ) else {
                return Ok(true);
            };
            Ok(fut
                .await?
                .is_none_or(|p| judge(ComponentKind::Prompt, &p.name, &p.meta)))
        }
        Component::Resource(uri) => {
            if let Some(fut) = router.dispatch_lookup_resource(
                server.clone(),
                ListResourcesContext::new(ctx.clone()),
                uri.into(),
            ) && let Some(r) = fut.await?
            {
                return Ok(judge(ComponentKind::Resource, &r.uri, &r.meta));
            }
            if let Some(fut) = router.dispatch_lookup_resource_template(
                server.clone(),
                ListResourceTemplatesContext::new(ctx.clone()),
                uri.into(),
            ) && let Some(t) = fut.await?
            {
                return Ok(judge(
                    ComponentKind::ResourceTemplate,
                    &t.uri_template,
                    &t.meta,
                ));
            }
            // Neither a listed resource nor a registered template matches, so
            // there is nothing for the policy to judge: refused, as a hidden
            // one is. Deferring to the handler here let through whatever the
            // handler chose to serve, and for resources the policy is the only
            // gate there is. A handler that normalizes URIs (case, a trailing
            // slash, percent-encoding) served a hidden `secret://x` as
            // `secret://x/`, which no exact-match lookup finds, and a URI under
            // a template the matcher can't parse slipped past the template's
            // own tags.
            Ok(true)
        }
    }
}

/// Whether the visibility policy (if any) hides a component the lookup found.
fn policy_hides(
    shared: &Shared,
    ctx: &RequestContext,
    kind: ComponentKind,
    id: &str,
    meta: &Map<String, Value>,
) -> bool {
    shared.visibility.as_ref().is_some_and(|policy| {
        !policy.is_visible(&VisibleComponent {
            kind,
            id,
            meta,
            request: ctx,
        })
    })
}

/// Why `completion/complete` must be refused for `reference`, if it must.
///
/// The ref is resolved for real, policy or not: "Invalid prompt name: `-32602`"
/// (completion.mdx §Error Handling) is a fact about the server, and it used to
/// hold only when a visibility policy happened to be installed. A `ref/resource`
/// names a listed resource's URI or a template's exact `uriTemplate` — matching
/// it as a concrete URI would let a template string accidentally expand against
/// another template. Hidden gets exactly the refusal unknown gets.
async fn completion_refusal<S: McpServerCore>(
    shared: &Shared,
    router: &MethodRouter<S>,
    server: &S,
    ctx: &RequestContext,
    reference: &neutral::CompletionReference,
) -> McpResult<Option<McpError>> {
    match reference {
        neutral::CompletionReference::Prompt { name } => {
            let unknown = McpError::invalid_params(format!("unknown prompt: {name}"));
            let Some(fut) = router.dispatch_lookup_prompt(
                server.clone(),
                ListPromptsContext::new(ctx.clone()),
                name.clone(),
            ) else {
                return Ok(Some(unknown));
            };
            Ok(match fut.await? {
                Some(p) if !policy_hides(shared, ctx, ComponentKind::Prompt, &p.name, &p.meta) => {
                    None
                }
                _ => Some(unknown),
            })
        }
        neutral::CompletionReference::ResourceTemplate { uri } => {
            let unknown = McpError::resource_not_found(uri.clone());
            if let Some(fut) = router.dispatch_lookup_resource(
                server.clone(),
                ListResourcesContext::new(ctx.clone()),
                uri.clone(),
            ) && let Some(r) = fut.await?
            {
                let hidden = policy_hides(shared, ctx, ComponentKind::Resource, &r.uri, &r.meta);
                return Ok(hidden.then_some(unknown));
            }
            let template = crate::catalog::find(
                |params| {
                    let listed = router.dispatch_list_resource_templates(
                        server.clone(),
                        ListResourceTemplatesContext::new(ctx.clone()),
                        params,
                    );
                    async move {
                        let Some(listed) = listed else {
                            return Ok((Vec::new(), None));
                        };
                        let page = listed.await?;
                        Ok((page.resource_templates, page.next_cursor))
                    }
                },
                |t: &neutral::ResourceTemplate| t.uri_template == *uri,
            )
            .await?;
            Ok(match template {
                Some(t)
                    if !policy_hides(
                        shared,
                        ctx,
                        ComponentKind::ResourceTemplate,
                        &t.uri_template,
                        &t.meta,
                    ) =>
                {
                    None
                }
                _ => Some(unknown),
            })
        }
        // A reference kind this build cannot resolve to a component cannot be
        // checked, so it is refused rather than waved through.
        other => Ok(Some(McpError::invalid_params(format!(
            "unsupported completion reference: {other:?}"
        )))),
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
    let sealer = &shared.sealer;
    let pending = &shared.pending;
    let method = req.method.as_str();
    let ctx = ctx.clone();
    let list_params = match parse_list_params(req.params.as_ref()) {
        Ok(params) => params,
        // Only a list method has a cursor to get wrong.
        Err(e) if method.ends_with("/list") => return error_response_for(id, &W::VERSION, &e),
        Err(_) => neutral::ListParams::new(),
    };
    match method {
        methods::request::TOOLS_LIST => {
            // Core Tasks (`2025-11-25`) advertise each tool's `taskSupport`.
            let task_support = if W::VERSION.has_core_tasks() && shared.tasks.is_some() {
                match super::legacy_tasks::any_tool_declares_task_support(&server, router, &ctx)
                    .await
                {
                    Ok(any_declared) => Some(any_declared),
                    Err(e) => return error_response_for(id, &W::VERSION, &e),
                }
            } else {
                None
            };
            let fut = router
                .dispatch_list_tools(server, ListToolsContext::new(ctx.clone()), list_params)
                .map(|fut| match task_support {
                    Some(any_declared) => fut
                        .map(move |r| {
                            r.map(|r| super::legacy_tasks::with_task_support(r, any_declared))
                        })
                        .boxed(),
                    None => fut,
                });
            let fut = with_visibility(fut, shared, &ctx, visibility::filter_tools);
            let fut = with_cache_default(fut, shared.cache.tools_list);
            finish::<_, W::ListTools>(id, method, &W::VERSION, fut).await
        }
        methods::request::TOOLS_CALL => {
            let (params, tool) =
                match prepare_tool::<S, W>(&server, router, req, &ctx, shared, id.clone()).await {
                    Ok(prepared) => prepared,
                    Err(response) => return *response,
                };
            // On 2026-07-28 a call reaches this path only once every
            // call-augmenting extension has passed on it, so a tool that can
            // only run as a task has nowhere to go.
            if W::VERSION.is_stateless()
                && tool.task_support == Some(neutral::TaskSupport::Required)
            {
                return super::task_required_refusal(shared, &ctx, id, &tool.name);
            }
            call_prepared_tool::<S, W>(server, router, req, &ctx, shared, id, params, tool).await
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
            if match hidden(
                shared,
                router,
                &server,
                &ctx,
                Component::Resource(&params.uri),
            )
            .await
            {
                Ok(hidden) => hidden,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            } {
                return error_response_for(
                    id,
                    &W::VERSION,
                    &McpError::resource_not_found(params.uri),
                );
            }
            let handle = match mrtr_handle::<W>(
                req,
                &ctx,
                sealer,
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
                    .with_progress(progress_reporter::<W>(req, &ctx))
                    .with_log(log_sender::<W>(&ctx, router.has_logging())),
                params,
            );
            let fut = with_cache_default(fut, shared.cache.resources_read);
            let subject = ctx.identity.principal_key();
            finish_mrtr::<_, W::ReadResource>(
                id,
                MrtrTurn {
                    method: &request_binding(req),
                    version: &W::VERSION,
                    subject,
                    handle: &handle,
                    sealer,
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
            if match hidden(
                shared,
                router,
                &server,
                &ctx,
                Component::Prompt(&params.name),
            )
            .await
            {
                Ok(hidden) => hidden,
                Err(e) => return error_response_for(id, &W::VERSION, &e),
            } {
                return error_response_for(
                    id,
                    &W::VERSION,
                    &McpError::invalid_params(format!("unknown prompt: {}", params.name)),
                );
            }
            let handle = match mrtr_handle::<W>(
                req,
                &ctx,
                sealer,
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
                    .with_progress(progress_reporter::<W>(req, &ctx))
                    .with_log(log_sender::<W>(&ctx, router.has_logging())),
                params,
            );
            let subject = ctx.identity.principal_key();
            finish_mrtr::<_, W::GetPrompt>(
                id,
                MrtrTurn {
                    method: &request_binding(req),
                    version: &W::VERSION,
                    subject,
                    handle: &handle,
                    sealer,
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
            // Completion is reachability-bearing like every other method that
            // names a component: autocompleting a hidden prompt's arguments
            // discloses that it exists, and its values. Hidden means
            // indistinguishable from absent, so this answers exactly what an
            // unknown ref would — the same refusal `prompts/get` gives above.
            // A reference kind this build cannot resolve to a component cannot
            // be visibility-checked, so it is refused rather than waved
            // through — the same answer `parse_complete_params` gives an
            // unknown `ref` type, which is the only way to reach it.
            // An unadvertised capability is `-32601` before anything else is
            // looked at, whatever the request names.
            if !router.has_completions() {
                return error_response_for(id, &W::VERSION, &McpError::method_not_found(method));
            }
            match completion_refusal(shared, router, &server, &ctx, &params.reference).await {
                Ok(None) => {}
                Ok(Some(refusal)) | Err(refusal) => {
                    return error_response_for(id, &W::VERSION, &refusal);
                }
            }
            let fut = router.dispatch_complete(server, CompleteContext::new(ctx), params);
            finish::<_, W::Complete>(id, method, &W::VERSION, fut).await
        }
        _ => unreachable!("dispatch_capability called with an unrouted method"),
    }
}

/// Run a `tools/call` whose tool [`prepare_tool`] already resolved and whose
/// arguments it already validated.
#[allow(clippy::too_many_arguments)]
pub(super) async fn call_prepared_tool<S: McpServerCore, W: WireFamily>(
    server: S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    shared: &Shared,
    id: RequestId,
    params: neutral::CallToolParams,
    tool: neutral::Tool,
) -> JsonRpcMessage {
    let sealer = &shared.sealer;
    let pending = &shared.pending;
    let ctx = ctx.clone();
    let handle = match mrtr_handle::<W>(req, &ctx, sealer, pending, shared.strict_elicitation_keys)
    {
        Ok(h) => h,
        Err(e) => return error_response_for(id, &W::VERSION, &e),
    };
    let fut = router.dispatch_call_tool(
        server,
        CallToolContext::new(ctx.clone())
            .with_client(handle.clone())
            .with_progress(progress_reporter::<W>(req, &ctx))
            .with_log(log_sender::<W>(&ctx, router.has_logging())),
        params,
    );
    let validators = shared.validators.clone();
    let fut = fut.map(
        |fut| -> BoxFuture<'static, McpResult<neutral::CallToolResult>> {
            Box::pin(async move {
                let result = match fut.await {
                    // On the stateful revisions there is no code a client
                    // could act on (capabilities are fixed at `initialize`),
                    // so a tool that needs one the client lacks reports it as
                    // a tool failure the model can read and route around.
                    Err(McpError::MissingRequiredCapability(capability)) if !W::MRTR => {
                        return Ok(neutral::CallToolResult::error(format!(
                            "this tool needs the client capability `{capability}`, which \
                             the client did not declare"
                        )));
                    }
                    result => result?,
                };
                validators.output(tool.output_schema.as_ref(), result)
            })
        },
    );
    let subject = ctx.identity.principal_key();
    finish_mrtr::<_, W::CallTool>(
        id,
        MrtrTurn {
            method: &request_binding(req),
            version: &W::VERSION,
            subject,
            handle: &handle,
            sealer,
            mrtr_enabled: W::MRTR,
        },
        fut,
    )
    .await
}

// Bind state to the operation as well as the principal. Canonical object
// ordering permits intermediaries to reorder JSON properties between rounds.
pub(super) async fn prepare_tool<S: McpServerCore, W: WireFamily>(
    server: &S,
    router: &MethodRouter<S>,
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    shared: &Shared,
    id: RequestId,
) -> Result<(neutral::CallToolParams, neutral::Tool), Box<JsonRpcMessage>> {
    if W::MRTR {
        validate_mrtr_envelope(req).map_err(|e| error_response_for(id.clone(), &W::VERSION, &e))?;
    }
    let params = parse_call_tool_params(req.params.as_ref())
        .map_err(|e| error_response_for(id.clone(), &W::VERSION, &e))?;
    let tool = match router.dispatch_lookup_tool(
        server.clone(),
        ListToolsContext::new(ctx.clone()),
        params.name.clone(),
    ) {
        Some(fut) => fut
            .await
            .map_err(|e| error_response_for(id.clone(), &W::VERSION, &e))?,
        None => None,
    };
    // "Protocol Errors: Standard JSON-RPC errors for issues like unknown
    // tools", with `-32602 Unknown tool: …` as the spec's own example. The
    // distinction is not cosmetic: a `CallToolResult { isError }` is the
    // channel a model is *meant* to read and self-correct from, and a name it
    // cannot invent its way out of does not belong there. A hidden tool
    // answers identically, so visibility stays indistinguishable from absence.
    let unknown = || {
        error_response_for(
            id.clone(),
            &W::VERSION,
            &McpError::invalid_params(format!("unknown tool: {}", params.name)),
        )
    };
    let tool = tool.ok_or_else(unknown)?;
    if shared.visibility.as_ref().is_some_and(|policy| {
        !policy.is_visible(&VisibleComponent {
            kind: ComponentKind::Tool,
            id: &tool.name,
            meta: &tool.meta,
            request: ctx,
        })
    }) {
        return Err(Box::new(unknown()));
    }
    // Declared scopes are checked here, before any path can act on the call:
    // a task-augmented call refused inside its handler would already have
    // answered `CreateTaskResult`, so its caller would get a failed task
    // instead of the step-up challenge. (`#[tool(scopes(…))]` also guards the
    // handler itself, for callers that reach it without a dispatcher.)
    let required: Vec<&str> = crate::visibility::declared_scopes(&tool.meta).collect();
    if !ctx.identity.has_scopes(&required) {
        let refusal =
            crate::router::scope_refusal(ctx, required.into_iter().map(String::from).collect());
        return Err(Box::new(ok_value(id, &W::CallTool::from(refusal))));
    }
    check_header_mirrors(ctx.extensions.get::<ObservedHeaders>(), &params, &tool)
        .map_err(|e| error_response_for(id.clone(), &W::VERSION, &e))?;
    shared
        .validators
        .validate(&tool.input_schema, &Value::Object(params.arguments.clone()))
        .map_err(|e| match e {
            // Arguments that miss the schema are a tool execution error the
            // model can read and correct (tools.mdx: "Input validation errors").
            McpError::InvalidParams(_) => ok_value(
                id.clone(),
                &W::CallTool::from(neutral::CallToolResult::error(e.to_string())),
            ),
            // A schema that does not compile is the server's bug. Reported as a
            // tool result, the model would retry different arguments forever
            // and the operator would see nothing.
            other => error_response_for(id.clone(), &W::VERSION, &other),
        })?;
    Ok((params, tool))
}

/// A digest of the request a `requestState` belongs to: "the method name and a
/// digest of its salient parameters". The params are RFC 8785 canonical JSON,
/// so a retry an intermediary or a client re-encoded (keys reordered, `1`
/// written back as `1.0`, a string escaped differently) still matches; sorting
/// keys alone left a retry carrying `1.0` failing verification forever.
fn request_binding(req: &JsonRpcRequest) -> String {
    let mut params = req.params.clone().unwrap_or(Value::Null);
    if let Some(map) = params.as_object_mut() {
        for key in ["_meta", "requestState", "inputResponses"] {
            map.remove(key);
        }
    }
    use sha2::{Digest, Sha256};
    let encoded =
        serde_json_canonicalizer::to_vec(&serde_json::json!([req.method.as_str(), params]))
            .expect("JSON request binding");
    Sha256::digest(encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
fn validate_mrtr_envelope(req: &JsonRpcRequest) -> McpResult<()> {
    if let Some(responses) = req.params.as_ref().and_then(|p| p.get("inputResponses"))
        && !responses
            .as_object()
            .is_some_and(|map| map.values().all(Value::is_object))
    {
        return Err(McpError::invalid_params(
            "inputResponses must map input keys to response objects",
        ));
    }
    if req
        .params
        .as_ref()
        .and_then(|p| p.get("requestState"))
        .is_some_and(|state| !state.is_string())
    {
        return Err(McpError::invalid_params("requestState must be a string"));
    }
    Ok(())
}

fn mrtr_handle<W: WireFamily>(
    req: &JsonRpcRequest,
    ctx: &RequestContext,
    sealer: &StateSealer,
    pending: &Arc<PendingRequests>,
    strict_keys: bool,
) -> Result<ClientHandle, McpError> {
    if !W::MRTR {
        // The legacy session gate ran before dispatch, so the session id is
        // present on this path; its absence means no client channel.
        return Ok(match ctx.extensions.get::<SessionId>() {
            Some(_) => ClientHandle::bidi(
                Route::for_request(&ctx.extensions, true),
                Arc::clone(pending),
                ctx.client_capabilities.clone(),
                W::VERSION,
            ),
            None => ClientHandle::unavailable("no session for inline bidirectional requests"),
        });
    }
    validate_mrtr_envelope(req)?;
    let fields: RawMrtrFields = req
        .params
        .as_ref()
        .map(|p| serde_json::from_value(p.clone()))
        .transpose()
        .map_err(|e| McpError::invalid_params(format!("invalid MRTR fields: {e}")))?
        .unwrap_or_default();
    let state_in = match &fields.request_state {
        Some(token) => Some(sealer.open(
            &request_binding(req),
            ctx.identity.principal_key().as_deref(),
            token,
        )?),
        None => None,
    };
    Ok(ClientHandle::mrtr(
        Route::for_request(&ctx.extensions, false),
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
    pub(super) sealer: &'a StateSealer,
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
        sealer,
        mrtr_enabled,
    } = turn;
    let Some(f) = fut else {
        return error_response_for(id, version, &McpError::method_not_found(method));
    };
    let outcome = f.await;
    // The handle, not the error, says whether the handler asked the client
    // for input. Code that adds context to errors
    // (`.map_err(|e| McpError::internal(format!("elicit: {e}")))?`), `anyhow`,
    // or a `#[tool]` turning the error into an `isError` result, all hid the
    // sentinel, and the client got a failure instead of the questions the
    // handle had already collected. A handler that swallows the abort and
    // returns anyway still gets the questions asked: it cannot have the
    // answers yet.
    if mrtr_enabled && handle.aborted() {
        let collected = handle.collected();
        let state_out = handle.state_out();
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
            match sealer.seal(method, subject.as_deref(), &data) {
                Ok(token) => {
                    result.insert("requestState".to_owned(), serde_json::json!(token));
                }
                Err(e) => return error_response_for(id, version, &e),
            }
        }
        return JsonRpcResponse::success(id, Value::Object(result)).into();
    }
    match outcome {
        Ok(result) => ok_value(id, &WIRE::from(result)),
        // The sentinel with no abort behind it: a handler returned it by hand.
        Err(McpError::InputRequired) if mrtr_enabled => error_response_for(
            id,
            version,
            &McpError::internal("MRTR abort recorded no input requests"),
        ),
        Err(e) => error_response_for(id, version, &e),
    }
}

/// Build the request's [`LogSender`]: live when the server enabled `logging`
/// AND the client opted in (the context's `log_level` carries the opt-in from
/// either the draft `_meta` key or the legacy session's `setLevel`). Routing
/// mirrors [`progress_reporter`].
fn log_sender<W: WireFamily>(ctx: &RequestContext, logging_enabled: bool) -> LogSender {
    let Some(min) = ctx.log_level.filter(|_| logging_enabled) else {
        return LogSender::disabled();
    };
    LogSender::new(min, Route::for_request(&ctx.extensions, !W::MRTR))
}

/// Build the request's [`ProgressReporter`]: live when the request carried a
/// `_meta.progressToken` (string or integer per the progress spec — anything
/// else is treated as absent, with a warning), inert otherwise. Notifications
/// route to the request's own stream; the legacy family may fall back to the
/// session `GET` stream, the draft never does.
pub(super) fn progress_reporter<W: WireFamily>(
    req: &JsonRpcRequest,
    ctx: &RequestContext,
) -> ProgressReporter {
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
    ProgressReporter::new(token.clone(), Route::for_request(&ctx.extensions, !W::MRTR))
}

#[cfg(test)]
mod header_mirror_tests {
    use super::*;
    use serde_json::json;

    /// A tool whose `inputSchema` annotates a nested property and a renamed
    /// one, next to an unannotated argument that shares the renamed header's
    /// name.
    fn tool() -> neutral::Tool {
        neutral::Tool::new(
            "route",
            json!({
                "type": "object",
                "properties": {
                    "cfg": {
                        "type": "object",
                        "properties": { "region": { "type": "string", "x-mcp-header": "Region" } }
                    },
                    "target_zone": { "type": "string", "x-mcp-header": "Zone" },
                    "zone": { "type": "string" },
                    "count": { "type": "integer", "x-mcp-header": "Count" }
                }
            }),
        )
    }

    fn check(arguments: Value, observed: Value) -> McpResult<()> {
        let params = neutral::CallToolParams::new(
            "route",
            arguments.as_object().cloned().unwrap_or_default(),
        );
        let observed = ObservedHeaders(
            observed
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().map(str::to_owned)))
                .collect(),
        );
        check_header_mirrors(Some(&observed), &params, &tool())
    }

    #[test]
    fn a_nested_mirror_is_checked_against_the_nested_value() {
        let args = json!({ "cfg": { "region": "us-east" } });
        assert!(check(args.clone(), json!({ "region": "us-east" })).is_ok());
        // The split-brain the rule exists for: routed as eu-west, run as us-east.
        assert!(check(args, json!({ "region": "eu-west" })).is_err());
    }

    /// The header mirrors the property its annotation is on, not whichever
    /// argument happens to share the header's name.
    #[test]
    fn a_renamed_mirror_is_checked_against_its_own_property() {
        let args = json!({ "target_zone": "a", "zone": "b" });
        assert!(check(args.clone(), json!({ "zone": "a" })).is_ok());
        assert!(check(args, json!({ "zone": "b" })).is_err());
    }

    #[test]
    fn missing_extra_and_unreadable_mirrors_are_mismatches() {
        let args = json!({ "target_zone": "a" });
        assert!(check(args.clone(), json!({})).is_err(), "missing");
        assert!(
            check(json!({}), json!({ "zone": "a" })).is_err(),
            "no body value"
        );
        assert!(
            check(args, json!({ "zone": null })).is_err(),
            "invalid characters"
        );
        // A header the tool doesn't declare is someone else's business.
        assert!(check(json!({}), json!({ "trace": "x" })).is_ok());
    }

    #[test]
    fn a_base64_mirror_is_decoded_first() {
        let args = json!({ "target_zone": "Hello, 世界" });
        assert!(check(args, json!({ "zone": "=?base64?SGVsbG8sIOS4lueVjA==?=" })).is_ok());
    }

    /// "Servers SHOULD compare the header value and the body value
    /// numerically rather than as strings (e.g., `42.0` and `42` are
    /// considered equal)."
    #[test]
    fn integers_compare_numerically() {
        assert!(check(json!({ "count": 42.0 }), json!({ "count": "42" })).is_ok());
        assert!(check(json!({ "count": 42 }), json!({ "count": "42.0" })).is_ok());
        assert!(check(json!({ "count": 42 }), json!({ "count": "43" })).is_err());
        assert!(check(json!({ "count": 42.5 }), json!({ "count": "42.5" })).is_err());
    }

    #[test]
    fn no_observed_headers_means_no_mirroring_in_effect() {
        let params = neutral::CallToolParams::new("route", Map::new());
        assert!(check_header_mirrors(None, &params, &tool()).is_ok());
    }
}
