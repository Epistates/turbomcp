//! [`VersionDispatcher`]: the `tower::Service` that turns a [`JsonRpcMessage`]
//! into a typed handler call and back.
//!
//! It lives here (not in `turbomcp-protocol`, as an early draft of the plan had
//! it) because it is generic over the user's [`McpServerCore`], which sits above
//! the protocol layer — putting it here keeps the dependency graph acyclic while
//! concentrating *all* per-version branching in one place. Above it (RPC
//! middleware) and below it (typed handlers) are version-agnostic.
//!
//! Per-version status (Phase 5): both paths are live. The modern
//! `2026-07-28` path is stateless (version in each request's `_meta`); the
//! legacy `2025-11-25` path is stateful — `initialize` negotiates a version and
//! mints a session (via the transport-supplied internal session id, see
//! [`turbomcp_core::meta::internal`]), and later requests are dispatched with
//! the session's negotiated client info/capabilities injected into their
//! [`RequestContext`]. Both paths converge on the same neutral handlers; only
//! the wire types differ (selected via the private `WireFamily` trait).
//!
//! `_meta`→context extraction may still move to a `MetaExtractLayer` once
//! Auth/RateLimit need to observe it between layers (Phase 6/7).

use std::sync::Arc;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower::Service;

use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, McpError, ProtocolVersion, RequestContext, RequestId, meta,
};
use turbomcp_protocol::neutral::CachePolicy;
use turbomcp_protocol::{methods, version};
use turbomcp_service::{ProtocolError, mcp_to_jsonrpc_error, mcp_to_jsonrpc_error_for};

use crate::extension::{Extension, ExtensionRequest};
use crate::inflight::InFlightRegistry;
use crate::mrtr::{PendingRequests, StateSigner};
use crate::router::MethodRouter;
use crate::session::{SessionBackend, SessionStore};
use crate::subscriptions::{ServerNotifier, SubscriptionRegistry};
use crate::tasks::{TaskBackend, TaskStore};
use crate::traits::McpServerCore;

mod augment;
mod capability;
mod handshake;
mod legacy_tasks;
mod listen;
mod params;

use augment::try_augment_call;
use capability::{DraftWire, LegacyWire, dispatch_capability};
use handshake::{discover_response, handle_initialize};
use legacy_tasks::{
    handle_tasks_method, has_task_field, legacy_list_tools_with_task_support, task_augmented_call,
};
use listen::handle_subscriptions_listen;
use params::{
    build_context, extract_log_level, legacy_context, parse_set_level_params, parse_uri_param,
};

/// The protocol seam for a server: `Service<JsonRpcMessage>`.
///
/// Clone is cheap (the server clones per request; the router is shared behind an
/// `Arc`), so the dispatcher composes under per-connection `tower` stacks.
pub struct VersionDispatcher<S> {
    server: S,
    router: Arc<MethodRouter<S>>,
    supported: Vec<ProtocolVersion>,
    shared: Shared,
}

/// The dispatcher's shared per-server state — one `Arc` per store, grouped so
/// the deep handler call chain threads a single value instead of six (and so
/// cross-store coordination, like session-termination tearing down a session's
/// subscription routes, has one place to live). Cheap to clone (six `Arc`s).
#[derive(Clone)]
struct Shared {
    sessions: Arc<dyn SessionBackend>,
    tasks: Option<Arc<dyn TaskBackend>>,
    inflight: Arc<InFlightRegistry>,
    subs: Arc<SubscriptionRegistry>,
    signer: Arc<StateSigner>,
    pending: Arc<PendingRequests>,
    /// Registered draft extensions (PLAN D10), consulted for `server/discover`
    /// advertisement and modern-path method routing. One `Arc` to keep the
    /// per-request `Shared` clone cheap.
    extensions: Arc<Vec<Arc<dyn Extension>>>,
    /// Opt-in: treat an elicit key reused with a different shape as an error.
    strict_elicitation_keys: bool,
    /// Per-capability cache defaults (SEP-2549), applied to draft cacheable
    /// results whose handler didn't set a policy.
    cache: CachePolicies,
}

/// Per-capability cache defaults (SEP-2549) for the `2026-07-28` wire's
/// `ttlMs`/`cacheScope` fields — one [`CachePolicy`] per cacheable surface
/// (the four `*/list`s and `resources/read`; `server/discover` lost its cache
/// fields in the 2026-07-28 RC and is not configurable). The default is
/// [`CachePolicy::NO_CACHE`] everywhere (private + immediately stale —
/// exactly the pre-configuration behavior). A handler-set policy on a neutral
/// result wins over these defaults. The `2025-11-25` wire has no cache
/// fields, so this configuration is inert there.
///
/// For the common one-knob case a bare [`CachePolicy`] converts into a
/// uniform `CachePolicies`; chain the per-surface setters for granularity:
///
/// ```ignore
/// builder.cache_policy(CachePolicy::public(Duration::from_secs(60)));
/// builder.cache_policy(
///     CachePolicies::default().tools_list(CachePolicy::private(Duration::from_secs(30))),
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CachePolicies {
    pub(crate) tools_list: CachePolicy,
    pub(crate) resources_list: CachePolicy,
    pub(crate) resource_templates_list: CachePolicy,
    pub(crate) resources_read: CachePolicy,
    pub(crate) prompts_list: CachePolicy,
}

impl CachePolicies {
    /// The same policy for every cacheable surface.
    #[must_use]
    pub fn uniform(policy: CachePolicy) -> Self {
        Self {
            tools_list: policy,
            resources_list: policy,
            resource_templates_list: policy,
            resources_read: policy,
            prompts_list: policy,
        }
    }

    /// Set the `tools/list` policy.
    #[must_use]
    pub fn tools_list(mut self, policy: CachePolicy) -> Self {
        self.tools_list = policy;
        self
    }

    /// Set the `resources/list` policy.
    #[must_use]
    pub fn resources_list(mut self, policy: CachePolicy) -> Self {
        self.resources_list = policy;
        self
    }

    /// Set the `resources/templates/list` policy.
    #[must_use]
    pub fn resource_templates_list(mut self, policy: CachePolicy) -> Self {
        self.resource_templates_list = policy;
        self
    }

    /// Set the `resources/read` policy.
    #[must_use]
    pub fn resources_read(mut self, policy: CachePolicy) -> Self {
        self.resources_read = policy;
        self
    }

    /// Set the `prompts/list` policy.
    #[must_use]
    pub fn prompts_list(mut self, policy: CachePolicy) -> Self {
        self.prompts_list = policy;
        self
    }
}

impl Default for CachePolicies {
    fn default() -> Self {
        Self::uniform(CachePolicy::NO_CACHE)
    }
}

impl From<CachePolicy> for CachePolicies {
    fn from(policy: CachePolicy) -> Self {
        Self::uniform(policy)
    }
}

impl Shared {
    /// Reclaim every idle-expired session and tear down its legacy
    /// subscription routes in one place (the session store and the
    /// subscription registry don't know about each other). Called
    /// opportunistically at `initialize`, where new sessions are minted — a
    /// natural, cheap point to bound stale-session growth without a background
    /// task. A store with no idle timeout sweeps nothing.
    async fn sweep_idle_sessions(&self) {
        for id in self.sessions.sweep_expired().await {
            self.subs.legacy_remove(&id);
        }
    }

    /// Terminate one session: drop its state and its legacy subscription
    /// routes. Returns whether the session existed. Backs explicit `DELETE`
    /// session termination.
    async fn terminate_session(&self, id: &str) -> bool {
        let existed = self.sessions.remove(id).await;
        self.subs.legacy_remove(id);
        existed
    }
}

/// The [`SessionTerminator`](turbomcp_service::SessionTerminator) handle returned by
/// [`VersionDispatcher::session_terminator`]: shares the dispatcher's stores so
/// `DELETE` drops the session state and its subscription routes together.
#[derive(Clone)]
pub struct DispatcherSessionTerminator {
    shared: Shared,
}

impl turbomcp_service::SessionTerminator for DispatcherSessionTerminator {
    fn terminate<'a>(&'a self, session_id: &'a str) -> turbomcp_service::TerminateFuture<'a> {
        Box::pin(self.shared.terminate_session(session_id))
    }
}

impl<S: Clone> Clone for VersionDispatcher<S> {
    fn clone(&self) -> Self {
        Self {
            server: self.server.clone(),
            router: Arc::clone(&self.router),
            supported: self.supported.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl<S: McpServerCore> VersionDispatcher<S> {
    /// Build a dispatcher for `server` with `router`'s registered capabilities.
    /// The accepted version set is taken from [`McpServerCore::supported_versions`].
    #[must_use]
    pub fn new(server: S, router: MethodRouter<S>) -> Self {
        let supported = server.supported_versions().to_vec();
        Self {
            server,
            router: Arc::new(router),
            supported,
            shared: Shared {
                sessions: Arc::new(SessionStore::default()),
                tasks: None,
                inflight: Arc::new(InFlightRegistry::default()),
                subs: Arc::new(SubscriptionRegistry::default()),
                signer: Arc::new(StateSigner::new()),
                pending: Arc::new(PendingRequests::default()),
                extensions: Arc::new(Vec::new()),
                strict_elicitation_keys: false,
                cache: CachePolicies::default(),
            },
        }
    }

    /// A cloneable handle for publishing change notifications
    /// (`*_list_changed`, `resources/updated`) to every live subscription.
    #[must_use]
    pub fn notifier(&self) -> ServerNotifier {
        ServerNotifier::new(Arc::clone(&self.shared.subs))
    }

    /// A [`SessionTerminator`](turbomcp_service::SessionTerminator) handle for
    /// the HTTP transport: hand it to `HttpConfig::with_session_terminator` so a
    /// client `DELETE` ends its `2025-11-25` session (dropping the session state
    /// and its subscription routes). Without it, `DELETE` answers `405` (the
    /// spec permits refusing).
    #[must_use]
    pub fn session_terminator(&self) -> DispatcherSessionTerminator {
        DispatcherSessionTerminator {
            shared: self.shared.clone(),
        }
    }

    /// Drop every live `subscriptions/listen` subscription at graceful
    /// shutdown. The subscriptions spec (2026-07-28 RC) sends **no** closing
    /// response — the server ends a subscription by closing the underlying
    /// stream, which the HTTP transport does off its shutdown token. This
    /// clears the registry so no further notifications are routed.
    /// `run_http` wires it to the configured shutdown token automatically.
    pub fn close_subscriptions(&self) {
        self.shared.subs.close_all();
    }

    /// Opt in to strict elicitation keys: reusing an `elicit` key with a
    /// different request shape within one handler execution becomes an error
    /// (an idempotency lint) instead of a warning.
    #[must_use]
    pub fn strict_elicitation_keys(mut self) -> Self {
        self.shared.strict_elicitation_keys = true;
        self
    }

    /// Set the per-capability cache defaults (SEP-2549) advertised on draft
    /// cacheable results (`server/discover`, the four `*/list`s, and
    /// `resources/read`). Accepts a bare [`CachePolicy`] for a uniform policy
    /// or a [`CachePolicies`] for per-capability control. A handler-set policy
    /// on a neutral result wins over these defaults. Without this, every
    /// cacheable result advertises `ttlMs: 0` / `cacheScope: "private"`
    /// (immediately stale).
    #[must_use]
    pub fn with_cache_policy(mut self, cache: impl Into<CachePolicies>) -> Self {
        self.shared.cache = cache.into();
        self
    }

    /// Sign MRTR `requestState` with `key` instead of a per-process random
    /// secret. See [`ServerBuilder::with_state_key`](crate::ServerBuilder::with_state_key)
    /// for when this is required and how to source the key.
    #[must_use]
    pub fn with_state_key(mut self, key: [u8; 32]) -> Self {
        self.shared.signer = Arc::new(StateSigner::from_key(key));
        self
    }

    /// Enable core Tasks (`2025-11-25`): task-augmented `tools/call` plus
    /// `tasks/list|get|cancel|result`, advertised via the `tasks` capability
    /// at `initialize`. Tools then default to `execution.taskSupport:
    /// "optional"` in `tools/list`.
    ///
    /// Task-augmented calls run on spawned tasks, so the dispatcher must be
    /// driven inside a tokio runtime (all bundled transports do this).
    #[must_use]
    pub fn with_task_support(mut self) -> Self {
        self.shared.tasks = Some(Arc::new(TaskStore::default()));
        self
    }

    /// Register a draft [`Extension`] (PLAN D10): it is advertised in
    /// `server/discover` under `capabilities.extensions[id]` and owns its
    /// declared methods on the modern (`2026-07-28`) path. Extensions are
    /// draft-only — the legacy `2025-11-25` path serves its built-in
    /// equivalents (core Tasks via [`with_task_support`](Self::with_task_support)).
    #[must_use]
    pub fn with_extension(mut self, extension: Arc<dyn Extension>) -> Self {
        // `with_extension` runs at build time (rare); rebuild the shared `Arc`
        // so per-request `Shared` clones stay a single `Arc` bump.
        let mut extensions = Vec::clone(&self.shared.extensions);
        extensions.push(extension);
        self.shared.extensions = Arc::new(extensions);
        self
    }

    /// Evict a legacy session not seen within `timeout` (and tear down its
    /// subscription routes). Without this, sessions are bounded only by the
    /// store's LRU capacity. Call at build time, before serving.
    #[must_use]
    pub fn with_session_idle_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.shared.sessions = Arc::new(
            SessionStore::with_capacity(SessionStore::DEFAULT_CAPACITY)
                .with_idle_timeout(Some(timeout)),
        );
        self
    }

    /// Store legacy session state in `backend` instead of the bundled
    /// in-memory [`SessionStore`] — the seam for external session storage
    /// (e.g. Redis), so multiple instances can serve the same session.
    /// Replaces any prior store configuration
    /// ([`with_session_idle_timeout`](Self::with_session_idle_timeout) applies
    /// only to the bundled store).
    #[must_use]
    pub fn with_session_backend(mut self, backend: Arc<dyn SessionBackend>) -> Self {
        self.shared.sessions = backend;
        self
    }

    /// Enable core Tasks (`2025-11-25`) backed by `backend` instead of the
    /// bundled in-memory [`TaskStore`] — the seam for external task storage.
    #[must_use]
    pub fn with_task_backend(mut self, backend: Arc<dyn TaskBackend>) -> Self {
        self.shared.tasks = Some(backend);
        self
    }
}

impl<S: McpServerCore> Service<JsonRpcMessage> for VersionDispatcher<S> {
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Backpressure (bounded request queue) lands with the Phase 4
        // writer-actor; today the dispatcher is always ready.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, msg: JsonRpcMessage) -> Self::Future {
        let server = self.server.clone();
        let router = Arc::clone(&self.router);
        let supported = self.supported.clone();
        let shared = self.shared.clone();
        Box::pin(async move { handle(server, router, supported, shared, msg).await })
    }
}

async fn handle<S: McpServerCore>(
    server: S,
    router: Arc<MethodRouter<S>>,
    supported: Vec<ProtocolVersion>,
    shared: Shared,
    msg: JsonRpcMessage,
) -> Result<Option<JsonRpcMessage>, ProtocolError> {
    // JSON-RPC 2.0 §4: a frame declaring a version other than "2.0" is an
    // Invalid Request (`-32600`). A frame *omitting* the field is tolerated —
    // decode defaults it to "2.0" — so only explicit wrong versions land here.
    if !msg.has_valid_version() {
        return Ok(match msg {
            JsonRpcMessage::Request(req) => Some(
                JsonRpcResponse::error(
                    req.id,
                    JsonRpcError {
                        code: -32600,
                        message: "invalid jsonrpc version (expected \"2.0\")".to_owned(),
                        data: None,
                    },
                )
                .into(),
            ),
            // No id to answer with: drop the frame.
            JsonRpcMessage::Notification(_) | JsonRpcMessage::Response(_) => None,
        });
    }
    match msg {
        JsonRpcMessage::Request(req) => {
            // Track the request for `notifications/cancelled` while it
            // dispatches — but only on an identified connection (the serve
            // driver injects the id; HTTP cancels by closing the stream).
            let cancel = CancellationToken::new();
            let _guard = connection_id(req.params.as_ref())
                .map(|conn| shared.inflight.register(conn, &req.id, cancel.clone()));

            // `subscriptions/listen` is the one MCP request with no JSON-RPC
            // response: its stream begins with an acknowledged *notification*
            // via the connection's writer, so it can't share `handle_request`'s
            // always-respond contract.
            if req.method == methods::request::SUBSCRIPTIONS_LISTEN {
                return handle_subscriptions_listen(
                    &router,
                    &supported,
                    &shared.subs,
                    &shared.extensions,
                    &req,
                    &cancel,
                )
                .await;
            }

            let dispatch =
                handle_request(server, &router, &supported, &shared, req, cancel.clone());
            tokio::select! {
                // Cancelled mid-flight: drop the handler future and send
                // nothing (cancellation spec: "stop processing … not send a
                // response for the cancelled request").
                () = cancel.cancelled() => Ok(None),
                out = dispatch => Ok(Some(out?)),
            }
        }
        JsonRpcMessage::Notification(n) => {
            handle_notification(&shared.inflight, &shared.subs, &n);
            Ok(None)
        }
        JsonRpcMessage::Response(resp) => {
            // A client→server response answers a server-initiated inline bidi
            // request (legacy elicitation/sampling/roots): route it to the
            // awaiting handler. Unsolicited responses are ignored.
            if !shared.pending.complete(resp) {
                tracing::debug!("ignoring unsolicited client->server response");
            }
            Ok(None)
        }
    }
}

#[derive(Deserialize)]
struct RawCancelledParams {
    #[serde(rename = "requestId")]
    request_id: RequestId,
    #[serde(default)]
    reason: Option<String>,
}

fn handle_notification(
    inflight: &InFlightRegistry,
    subs: &SubscriptionRegistry,
    n: &JsonRpcNotification,
) {
    match n.method.as_str() {
        methods::notification::CANCELLED => {
            // Fire-and-forget per spec: malformed params, unknown ids, and
            // already-finished requests are all silently ignored.
            let Some(conn) = connection_id(n.params.as_ref()) else {
                tracing::debug!("notifications/cancelled without a connection; ignored");
                return;
            };
            let Some(parsed) = n
                .params
                .as_ref()
                .and_then(|p| serde_json::from_value::<RawCancelledParams>(p.clone()).ok())
            else {
                tracing::debug!("malformed notifications/cancelled; ignored");
                return;
            };
            // The id may name an in-flight request *or* a live subscription
            // (cancelling the `subscriptions/listen` request id is how a
            // stdio client closes its stream).
            let fired = inflight.cancel(conn, &parsed.request_id);
            let unsubscribed = subs.remove(conn, &parsed.request_id);
            tracing::debug!(
                request_id = ?parsed.request_id,
                reason = parsed.reason.as_deref().unwrap_or(""),
                fired,
                unsubscribed,
                "notifications/cancelled"
            );
        }
        methods::notification::INITIALIZED => {
            tracing::debug!("received notifications/initialized");
        }
        other => tracing::debug!(method = other, "unhandled notification"),
    }
}

async fn handle_request<S: McpServerCore>(
    server: S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    shared: &Shared,
    req: JsonRpcRequest,
    cancel: CancellationToken,
) -> Result<JsonRpcMessage, ProtocolError> {
    // The fields this path needs; `signer`/`pending` flow on into
    // `dispatch_capability` via `shared`.
    let Shared {
        sessions,
        tasks,
        subs,
        ..
    } = shared;
    let id = req.id.clone();
    let method = req.method.clone();

    // Extension-owned methods (PLAN D10) are draft-only: on the modern path
    // route them to the registered extension once the client has declared its
    // capability; the legacy path falls through to the built-in equivalents
    // (core Tasks) handled by the arms below.
    if let Some(ext) = shared
        .extensions
        .iter()
        .find(|e| e.methods().contains(&method.as_str()))
        .cloned()
        && matches!(
            classify_version(req.params.as_ref(), supported),
            VersionRoute::Modern
        )
    {
        let ctx = build_context(&req);
        if !context_declares_extension(&ctx, ext.id()) {
            // SEP-2663: a client that didn't declare the extension capability
            // gets `-32601` for the extension's methods.
            return Ok(error_response(id, &McpError::method_not_found(method)));
        }
        let connection_id = connection_id(req.params.as_ref()).map(str::to_owned);
        return Ok(ext
            .dispatch(ExtensionRequest {
                request: req,
                context: ctx,
                connection_id,
            })
            .await);
    }

    match method.as_str() {
        // Version-agnostic methods: a client may call these before it knows
        // which version to pin (discovery) or merely to probe liveness.
        methods::request::DISCOVER => Ok(discover_response(
            id,
            &server,
            router,
            supported,
            &shared.extensions,
        )),
        methods::request::PING => Ok(JsonRpcResponse::success(id, serde_json::json!({})).into()),

        // Stateful handshake (2025-11-25 and earlier).
        methods::request::INITIALIZE => {
            // Bound stale-session growth: reclaim idle sessions (and their
            // routes) whenever a new one is minted.
            shared.sweep_idle_sessions().await;
            let tasks_enabled = tasks.is_some() && router.has_tools();
            let reply = handle_initialize(
                &server,
                router,
                supported,
                sessions.as_ref(),
                tasks_enabled,
                &req,
            )
            .await;
            // A successfully initialized session gets a delivery route, so
            // list_changed notifications can reach it from the start.
            if matches!(&reply, JsonRpcMessage::Response(r) if r.error.is_none())
                && let Some(sid) = session_id(req.params.as_ref())
            {
                subs.legacy_touch(sid, connection_id(req.params.as_ref()));
            }
            Ok(reply)
        }

        // Version-gated methods (every capability `*/list|read|get|call|complete`).
        methods::request::TOOLS_LIST
        | methods::request::TOOLS_CALL
        | methods::request::RESOURCES_LIST
        | methods::request::RESOURCES_TEMPLATES_LIST
        | methods::request::RESOURCES_READ
        | methods::request::PROMPTS_LIST
        | methods::request::PROMPTS_GET
        | methods::request::COMPLETION_COMPLETE => {
            match classify_version(req.params.as_ref(), supported) {
                VersionRoute::Modern => {
                    let mut ctx = build_context(&req);
                    ctx.cancellation = cancel;
                    // Draft logging opt-in: an unrecognized level rejects the
                    // request (logging spec §Error Handling).
                    match extract_log_level(req.params.as_ref()) {
                        Ok(level) => ctx.log_level = level,
                        Err(e) => return Ok(error_response(id, &e)),
                    }
                    // Draft Tasks extension (SEP-2663): a `tools/call` from a
                    // client that declared a call-augmenting extension may be
                    // converted into a task (`CreateTaskResult`) instead of
                    // running synchronously. The extension decides per call; a
                    // `None` here falls through to the normal dispatch.
                    if method == methods::request::TOOLS_CALL
                        && let Some(resp) =
                            try_augment_call(&server, router, &req, &ctx, &shared.extensions, &id)
                                .await
                    {
                        return Ok(resp);
                    }
                    Ok(
                        dispatch_capability::<S, DraftWire>(server, router, &req, &ctx, shared, id)
                            .await,
                    )
                }
                VersionRoute::Legacy => {
                    let mut ctx = match legacy_context(sessions.as_ref(), &req).await? {
                        Ok(ctx) => ctx,
                        Err(response) => return Ok(response),
                    };
                    ctx.cancellation = cancel;
                    // Keep the session's stdio delivery route fresh for
                    // server-initiated notifications.
                    if let Some(sid) = session_id(req.params.as_ref()) {
                        subs.legacy_touch(sid, connection_id(req.params.as_ref()));
                    }
                    // Core Tasks hooks (2025-11-25 only): augmented tools/call
                    // detaches into a task; tools/list advertises taskSupport.
                    if let Some(store) = tasks {
                        if method == methods::request::TOOLS_CALL
                            && has_task_field(req.params.as_ref())
                        {
                            return Ok(
                                task_augmented_call(server, router, store, ctx, &req, id).await
                            );
                        }
                        if method == methods::request::TOOLS_LIST {
                            return Ok(legacy_list_tools_with_task_support(
                                server, router, &req, ctx, id,
                            )
                            .await);
                        }
                    }
                    Ok(
                        dispatch_capability::<S, LegacyWire>(
                            server, router, &req, &ctx, shared, id,
                        )
                        .await,
                    )
                }
                VersionRoute::Unsupported(requested) => {
                    Ok(unsupported_version(id, requested, supported))
                }
            }
        }

        // Legacy resource subscriptions (2025-11-25; the draft subscribes via
        // `subscriptions/listen` instead).
        methods::request::RESOURCES_SUBSCRIBE | methods::request::RESOURCES_UNSUBSCRIBE => {
            match classify_version(req.params.as_ref(), supported) {
                VersionRoute::Legacy => {
                    if let Err(response) = legacy_context(sessions.as_ref(), &req).await? {
                        return Ok(response);
                    }
                    if !router.has_resources() {
                        return Ok(error_response(id, &McpError::method_not_found(method)));
                    }
                    let uri = match parse_uri_param(req.params.as_ref(), &method) {
                        Ok(uri) => uri,
                        Err(e) => return Ok(error_response(id, &e)),
                    };
                    // `legacy_context` proved the session id is present.
                    let sid = session_id(req.params.as_ref()).unwrap_or_default();
                    if method == methods::request::RESOURCES_SUBSCRIBE {
                        subs.legacy_subscribe(sid, connection_id(req.params.as_ref()), uri);
                    } else {
                        subs.legacy_unsubscribe(sid, &uri);
                    }
                    Ok(JsonRpcResponse::success(id, serde_json::json!({})).into())
                }
                VersionRoute::Modern => Ok(error_response(id, &McpError::method_not_found(method))),
                VersionRoute::Unsupported(requested) => {
                    Ok(unsupported_version(id, requested, supported))
                }
            }
        }

        // Legacy per-session log-level opt-in (2025-11-25; the draft replaced
        // the RPC with the per-request `_meta` `logLevel` key).
        methods::request::LOGGING_SET_LEVEL => {
            match classify_version(req.params.as_ref(), supported) {
                VersionRoute::Legacy => {
                    if let Err(response) = legacy_context(sessions.as_ref(), &req).await? {
                        return Ok(response);
                    }
                    if !router.has_logging() {
                        return Ok(error_response(id, &McpError::method_not_found(method)));
                    }
                    let level = match parse_set_level_params(req.params.as_ref()) {
                        Ok(level) => level,
                        Err(e) => return Ok(error_response(id, &e)),
                    };
                    // `legacy_context` proved the session id is present.
                    let sid = session_id(req.params.as_ref()).unwrap_or_default();
                    sessions.set_log_level(sid, level).await;
                    Ok(JsonRpcResponse::success(id, serde_json::json!({})).into())
                }
                VersionRoute::Modern => Ok(error_response(id, &McpError::method_not_found(method))),
                VersionRoute::Unsupported(requested) => {
                    Ok(unsupported_version(id, requested, supported))
                }
            }
        }

        // Core Tasks methods (2025-11-25; the draft serves Tasks as an
        // extension instead — Phase 8).
        methods::request::TASKS_LIST
        | methods::request::TASKS_GET
        | methods::request::TASKS_CANCEL
        | methods::request::TASKS_RESULT => {
            match classify_version(req.params.as_ref(), supported) {
                VersionRoute::Legacy => {
                    // Same session gate as every other legacy method.
                    if let Err(response) = legacy_context(sessions.as_ref(), &req).await? {
                        return Ok(response);
                    }
                    let Some(store) = tasks else {
                        return Ok(error_response(id, &McpError::method_not_found(method)));
                    };
                    // `legacy_context` proved the session id is present.
                    let sid = session_id(req.params.as_ref())
                        .unwrap_or_default()
                        .to_owned();
                    Ok(handle_tasks_method(store, &sid, method.as_str(), &req, id).await)
                }
                VersionRoute::Modern => Ok(error_response(id, &McpError::method_not_found(method))),
                VersionRoute::Unsupported(requested) => {
                    Ok(unsupported_version(id, requested, supported))
                }
            }
        }

        other => Ok(error_response(id, &McpError::method_not_found(other))),
    }
}

// ---- version routing ---------------------------------------------------------

enum VersionRoute {
    Modern,
    Legacy,
    /// Requested version (or `None` if absent) is not supported.
    Unsupported(Option<String>),
}

fn classify_version(params: Option<&Value>, supported: &[ProtocolVersion]) -> VersionRoute {
    match version::request_protocol_version(params) {
        Some(v) if !supported.contains(&v) => {
            VersionRoute::Unsupported(Some(v.as_str().to_owned()))
        }
        Some(ProtocolVersion::V2025_11_25) => VersionRoute::Legacy,
        Some(_) => VersionRoute::Modern,
        // Missing version on a stateless (modern) method: per PLAN §4.9 we treat
        // absence as unsupported and return the server's version list, so a
        // client that omitted `_meta` can re-issue with a known version.
        None => VersionRoute::Unsupported(None),
    }
}

// ---- transport-asserted identifiers --------------------------------------------

/// Read the transport-asserted session id from a request's `params._meta`.
/// Transports sanitize inbound messages before injecting this key, so its
/// presence is trustworthy in-process (see [`meta::internal`]).
fn session_id(params: Option<&Value>) -> Option<&str> {
    params?
        .get("_meta")?
        .get(meta::internal::SESSION_ID)?
        .as_str()
}

/// Read the driver-asserted connection id from `params._meta` (same trust
/// model as [`session_id`]: the boundary sanitizes before injecting).
fn connection_id(params: Option<&Value>) -> Option<&str> {
    params?
        .get("_meta")?
        .get(meta::internal::CONNECTION_ID)?
        .as_str()
}

/// Whether the request's per-request client capabilities declare `ext_id` under
/// `extensions` (SEP-2663 capability negotiation). The draft client stamps its
/// capabilities into `_meta` (lifted into [`RequestContext::client_capabilities`]
/// by [`build_context`]).
fn context_declares_extension(ctx: &RequestContext, ext_id: &str) -> bool {
    ctx.client_capabilities
        .as_ref()
        .and_then(|caps| caps.get("extensions"))
        .and_then(Value::as_object)
        .is_some_and(|exts| exts.contains_key(ext_id))
}

/// Serialize a wire result into a success response, mapping the (practically
/// impossible) serialization failure to an internal error rather than panicking.
fn ok_value<T: Serialize>(id: RequestId, value: &T) -> JsonRpcMessage {
    match serde_json::to_value(value) {
        Ok(v) => JsonRpcResponse::success(id, v).into(),
        Err(e) => error_response(id, &McpError::internal(format!("serialize result: {e}"))),
    }
}

/// Render `err` for a request whose version isn't known (or isn't relevant to
/// the code): uses the current revision's mapping. Prefer
/// [`error_response_for`] anywhere the negotiated version is in hand.
fn error_response(id: RequestId, err: &McpError) -> JsonRpcMessage {
    JsonRpcResponse::error(id, mcp_to_jsonrpc_error(err)).into()
}

/// Render `err` as `version` spells it — resource-not-found is the one
/// version-split code (`-32002` through `2025-11-25`, `-32602` from the
/// 2026-07-28 RC on).
fn error_response_for(id: RequestId, version: &ProtocolVersion, err: &McpError) -> JsonRpcMessage {
    JsonRpcResponse::error(id, mcp_to_jsonrpc_error_for(err, version)).into()
}

/// `-32003` Missing Required Client Capability (SEP-2663): the client requested
/// an extension's behavior without declaring its capability. The `data` names
/// the required extension so the client can re-declare and retry.
fn missing_capability_response(id: RequestId, extension_id: &str) -> JsonRpcMessage {
    let err = JsonRpcError {
        code: -32003,
        message: "missing required client capability".to_owned(),
        data: Some(serde_json::json!({
            "requiredCapabilities": { "extensions": { extension_id: {} } }
        })),
    };
    JsonRpcResponse::error(id, err).into()
}

fn unsupported_version(
    id: RequestId,
    requested: Option<String>,
    supported: &[ProtocolVersion],
) -> JsonRpcMessage {
    let err = ProtocolError::UnsupportedVersion {
        requested,
        supported: supported.iter().map(|v| v.as_str().to_owned()).collect(),
    };
    err.into_response(id).into()
}
