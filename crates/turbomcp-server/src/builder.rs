//! [`ServerBuilder`]: assemble a server value and the capabilities it implements
//! into a [`VersionDispatcher`] ready to hand to a transport.
//!
//! The builder is transport- and codec-agnostic: it produces the
//! `tower::Service<McpRequest>`, and [`layer`](ServerBuilder::layer) /
//! [`serve`](ServerBuilder::serve) hand that to RPC middleware and a
//! transport.
//!
//! Dispatcher settings are recorded by name and applied in order by
//! [`build`](ServerBuilder::build), each through the [`VersionDispatcher`]
//! method of the same meaning, so a setting is defined in exactly one place.
//!
//! [`ServerBuilder::new`] (or [`IntoServerBuilder::into_server`], the same
//! thing as a method) starts from the capabilities the server registers
//! ([`McpServerCore::register`], which `#[server]` writes from its markers).
//! Chain `with_tools()` etc. to add more.
//!
//! This used to be two things: the macro emitted an inherent `into_server`
//! that registered its capabilities, while the trait method, which is what
//! generic code resolves to, returned an empty router. A helper generic over
//! `McpServerCore` deployed every server with no tools.

use std::sync::Arc;

use crate::dispatcher::{CachePolicies, VersionDispatcher};
use crate::extension::Extension;
use crate::router::MethodRouter;
use crate::session::SessionBackend;
use crate::tasks::TaskBackend;
use crate::traits::{McpServerCore, WithCompletions, WithPrompts, WithResources, WithTools};

/// Unbounded in `S`, and reports which seams have been overridden rather than
/// the trait objects behind them.
impl<S> core::fmt::Debug for ServerBuilder<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let settings: Vec<&str> = self.settings.iter().map(|(name, _)| *name).collect();
        f.debug_struct("ServerBuilder")
            .field("router", &self.router)
            .field("settings", &settings)
            .finish_non_exhaustive()
    }
}

/// Assembles a server and its [`MethodRouter`] into a [`VersionDispatcher`].
pub struct ServerBuilder<S> {
    server: S,
    router: MethodRouter<S>,
    /// Dispatcher settings, by the builder method that made them, applied in
    /// order by [`build`](Self::build). Recorded rather than applied so a
    /// builder about to be mounted, whose dispatcher is the composite's, can
    /// be refused by name instead of silently losing them.
    settings: Vec<(&'static str, Setting<S>)>,
}

/// One recorded dispatcher setting.
type Setting<S> = Box<dyn FnOnce(VersionDispatcher<S>) -> VersionDispatcher<S> + Send>;

impl<S: McpServerCore> ServerBuilder<S> {
    /// Start from `server` with the capabilities it registers
    /// ([`McpServerCore::register`]).
    #[must_use]
    pub fn new(server: S) -> Self {
        Self {
            server,
            router: S::register(MethodRouter::new()),
            settings: Vec::new(),
        }
    }

    /// Start from a server and a router built for it elsewhere: a server
    /// whose capabilities are known only at run time (a proxied upstream's,
    /// say) rather than from [`McpServerCore::register`].
    #[must_use]
    pub fn from_parts(server: S, router: MethodRouter<S>) -> Self {
        Self {
            server,
            router,
            settings: Vec::new(),
        }
    }

    /// Set the cache defaults (SEP-2549) advertised on draft cacheable
    /// results: a bare [`CachePolicy`](turbomcp_protocol::neutral::CachePolicy)
    /// for a uniform policy, or a [`CachePolicies`] for per-capability
    /// control. See [`VersionDispatcher::with_cache_policy`].
    #[must_use]
    pub fn cache_policy(self, cache: impl Into<CachePolicies>) -> Self {
        let cache = cache.into();
        self.setting("cache_policy", move |d| d.with_cache_policy(cache))
    }

    /// Expire a legacy (`2025-11-25`) session not used within `timeout`
    /// (default [`SessionStore::DEFAULT_IDLE_TIMEOUT`], an hour), tearing down
    /// its subscription routes and `GET` stream (see
    /// [`VersionDispatcher::with_session_idle_timeout`]).
    ///
    /// [`SessionStore::DEFAULT_IDLE_TIMEOUT`]: crate::SessionStore::DEFAULT_IDLE_TIMEOUT
    #[must_use]
    pub fn session_idle_timeout(self, timeout: std::time::Duration) -> Self {
        self.setting("session_idle_timeout", move |d| {
            d.with_session_idle_timeout(timeout)
        })
    }

    /// Enable core Tasks (`2025-11-25`): task-augmented `tools/call` plus
    /// `tasks/list|get|cancel|result`. See
    /// [`VersionDispatcher::with_task_support`]. Meaningful only alongside a
    /// registered tools capability.
    #[must_use]
    pub fn with_tasks(self) -> Self {
        self.setting("with_tasks", VersionDispatcher::with_task_support)
    }

    /// Enable core Tasks backed by a custom [`TaskBackend`] instead of the
    /// bundled in-memory store — the seam for external task storage. See
    /// [`VersionDispatcher::with_task_backend`]. Implies
    /// [`with_tasks`](Self::with_tasks). The `2026-07-28` Tasks extension can
    /// be handed the same backend (`TasksExtension::backend`), so one
    /// registry fronts both wires.
    #[must_use]
    pub fn with_task_backend(self, backend: Arc<dyn TaskBackend>) -> Self {
        self.setting("with_task_backend", move |d| d.with_task_backend(backend))
    }

    /// Store legacy (`2025-11-25`) session state in a custom
    /// [`SessionBackend`] instead of the bundled in-memory store — the seam
    /// for external session storage (e.g. Redis), so sessions outlive a
    /// restart and any replica a session is routed to can find it. The
    /// legacy wire still needs sticky routing on `Mcp-Session-Id` (see
    /// [`SessionBackend`]). When set,
    /// [`session_idle_timeout`](Self::session_idle_timeout) is ignored
    /// (eviction policy belongs to the backend). See
    /// [`VersionDispatcher::with_session_backend`].
    #[must_use]
    pub fn with_session_backend(self, backend: Arc<dyn SessionBackend>) -> Self {
        self.setting("with_session_backend", move |d| {
            d.with_session_backend(backend)
        })
    }

    /// Opt in to strict elicitation keys: reusing an `elicit` key with a
    /// different request shape in one handler execution is an error, not a
    /// warning. See [`VersionDispatcher::strict_elicitation_keys`].
    #[must_use]
    pub fn strict_elicitation_keys(self) -> Self {
        self.setting(
            "strict_elicitation_keys",
            VersionDispatcher::strict_elicitation_keys,
        )
    }

    /// Enable the `logging` capability: handlers gain a live `ctx.log` when
    /// the client opts in (`logging/setLevel` per session on `2025-11-25`;
    /// per-request `_meta` `io.modelcontextprotocol/logLevel` on the draft,
    /// where SEP-2577 deprecates the feature — prefer `stderr`/OpenTelemetry
    /// for new draft-only servers). Without the opt-in, `ctx.log` drops
    /// everything, per the logging spec's MUST NOT.
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.router = self.router.with_logging();
        self
    }

    /// Register a draft [`Extension`] (PLAN D10): advertised in
    /// `server/discover` under `capabilities.extensions[id]` and owning its
    /// declared methods on the modern (`2026-07-28`) path. The reference
    /// extension is the draft Tasks extension (`turbomcp-ext-tasks`).
    #[must_use]
    pub fn with_extension(self, extension: Arc<dyn Extension>) -> Self {
        self.setting("with_extension", move |d| d.with_extension(extension))
    }

    /// Install a [`VisibilityPolicy`](crate::VisibilityPolicy) — progressive
    /// disclosure. Components it hides are dropped from every list *and*
    /// answered as though they did not exist, since filtering a list without
    /// refusing the call would be theatre.
    ///
    /// ```
    /// # use turbomcp_server::{IntoServerBuilder, McpServerCore, Visibility};
    /// # use turbomcp_core::Implementation;
    /// # use std::sync::Arc;
    /// # #[derive(Clone)] struct MyServer;
    /// # impl McpServerCore for MyServer {
    /// #     fn server_info(&self) -> Implementation { Implementation::new("s", "1.0") }
    /// # }
    /// let dispatcher = MyServer
    ///     .into_server()
    ///     .with_visibility(Arc::new(
    ///         Visibility::new()
    ///             .hiding_tagged(["internal"])
    ///             .requiring_declared_scopes(),
    ///     ))
    ///     .build();
    /// ```
    ///
    /// See [the module docs](crate::visibility) for the cost (one list per
    /// guarded call) and for writing a policy of your own.
    #[must_use]
    pub fn with_visibility(self, policy: Arc<dyn crate::VisibilityPolicy>) -> Self {
        self.setting("with_visibility", move |d| d.with_visibility(policy))
    }

    /// Run `handler` when a client reports that its roots changed
    /// (`notifications/roots/list_changed`).
    ///
    /// This is the only list-changed notification that travels *client→server*,
    /// and without an observer it was logged and dropped: a server could read
    /// the client's roots once, cache them, and never hear that they moved —
    /// which is the whole reason a client declares `roots.listChanged`.
    ///
    /// The handler's job is to *invalidate*, not to re-read. A notification
    /// carries no [`ClientHandle`](crate::ClientHandle), so the new roots are
    /// fetched with `ctx.client.list_roots(…)` from the next request that needs
    /// them. It runs inline on the dispatch path, so keep it cheap.
    ///
    /// The [`RequestContext`](turbomcp_core::RequestContext) identifies whose
    /// roots moved (identity and trace context from the notification's `_meta`).
    #[must_use]
    pub fn on_roots_changed(
        self,
        handler: impl Fn(&turbomcp_core::RequestContext) + Send + Sync + 'static,
    ) -> Self {
        let handler: Arc<crate::dispatcher::RootsChangedHandler> = Arc::new(handler);
        self.setting("on_roots_changed", move |d| d.on_roots_changed(handler))
    }

    /// Tell `observer` when each stateful session ends, and how long it
    /// lived: `turbomcp-telemetry`'s `MetricsLayer` records
    /// `mcp.server.session.duration` from it, and a per-session proxy
    /// upstream closes on it. Observers accumulate: each one added is told.
    #[must_use]
    pub fn observe_sessions(self, observer: Arc<dyn turbomcp_service::SessionObserver>) -> Self {
        self.setting("observe_sessions", move |d| d.observe_sessions(observer))
    }

    /// Run every operation through `interceptor`, inside any added before
    /// it: typed middleware over the neutral request and result, on every
    /// revision and every path a call takes. See [`crate::intercept`].
    #[must_use]
    pub fn intercept(mut self, interceptor: Arc<dyn crate::Interceptor>) -> Self {
        self.router = self.router.with_interceptor(interceptor);
        self
    }

    /// Carry change notifications between replicas over `bus`: the
    /// [`ServerNotifier`](crate::ServerNotifier) publishes each change to
    /// it, and every replica delivers what it carries to the subscriptions
    /// it holds. See [the `bus` module](crate::bus).
    #[must_use]
    pub fn with_notification_bus(self, bus: Arc<dyn crate::NotificationBus>) -> Self {
        self.setting("with_notification_bus", move |d| {
            d.with_notification_bus(bus)
        })
    }

    /// The `2025-11-25` `execution.taskSupport` a tool that declares none
    /// gets once Tasks are on (default `optional`, every tool may run as a
    /// task). `TaskSupport::Forbidden` makes tasks opt-in per tool
    /// (`#[tool(task)]`), which is the spec's own default. See
    /// [`VersionDispatcher::undeclared_task_support`].
    #[must_use]
    pub fn undeclared_task_support(self, support: turbomcp_protocol::neutral::TaskSupport) -> Self {
        self.setting("undeclared_task_support", move |d| {
            d.undeclared_task_support(support)
        })
    }

    /// Cancel any call still running after `limit` and answer it with a
    /// timeout error (`-32000`), so a hung handler can't hold an admission
    /// slot for ever. The handler is dropped and its cancellation fires.
    /// Applies to each request, so a multi-round (`2026-07-28`) call gets
    /// `limit` per round, and a task-augmented call to returning its task, not
    /// to the task's work (whose bound is its TTL). Default: no limit.
    #[must_use]
    pub fn call_timeout(self, limit: std::time::Duration) -> Self {
        self.setting("call_timeout", move |d| d.call_timeout(limit))
    }

    /// How many compiled tool schemas to keep (default 256). Every
    /// `tools/call` validates its arguments, and a structured result its
    /// output, against a validator compiled once per distinct schema; a
    /// server or gateway offering more distinct schemas than this recompiles
    /// on churn.
    #[must_use]
    pub fn schema_cache_capacity(self, capacity: u64) -> Self {
        self.setting("schema_cache_capacity", move |d| {
            d.schema_cache_capacity(capacity)
        })
    }

    /// Send clients a reference instead of an internal error's text.
    ///
    /// An internal error is written for the operator, and often wraps
    /// something a client should not see: a SQL statement, a hostname, a
    /// connection string. With this set, every internal error the server
    /// answers, as a JSON-RPC `-32603` or as the text of a tool's `isError`
    /// result, reads `internal error (ref: <id>)`, and the original is logged
    /// through `tracing` at `error` under the same `error_ref`. Errors a tool
    /// meant the model to read (`McpError::tool_execution_failed`, invalid
    /// arguments) are unchanged.
    #[must_use]
    pub fn mask_internal_errors(self) -> Self {
        self.setting(
            "mask_internal_errors",
            VersionDispatcher::mask_internal_errors,
        )
    }

    /// Register the `tools/*` capability (requires `S: WithTools`).
    #[must_use]
    pub fn with_tools(mut self) -> Self
    where
        S: WithTools,
    {
        self.router = self.router.with_tools();
        self
    }

    /// Register the `resources/*` capability (requires `S: WithResources`).
    #[must_use]
    pub fn with_resources(mut self) -> Self
    where
        S: WithResources,
    {
        self.router = self.router.with_resources();
        self
    }

    /// Register the `prompts/*` capability (requires `S: WithPrompts`).
    #[must_use]
    pub fn with_prompts(mut self) -> Self
    where
        S: WithPrompts,
    {
        self.router = self.router.with_prompts();
        self
    }

    /// Register the `completion/complete` capability (requires `S: WithCompletions`).
    #[must_use]
    pub fn with_completions(mut self) -> Self
    where
        S: WithCompletions,
    {
        self.router = self.router.with_completions();
        self
    }

    /// Seal MRTR `requestState` with a key you supply, rather than the
    /// per-process random secret used by default. The same as
    /// [`with_state_keys`](Self::with_state_keys)`(key, [])`.
    ///
    /// ```
    /// # use turbomcp_server::{IntoServerBuilder, McpServerCore};
    /// # use turbomcp_core::Implementation;
    /// # #[derive(Clone)]
    /// # struct MyServer;
    /// # impl McpServerCore for MyServer {
    /// #     fn server_info(&self) -> Implementation { Implementation::new("s", "1.0") }
    /// # }
    /// # fn load_from_secret_manager() -> [u8; 32] { [0u8; 32] }
    /// let key: [u8; 32] = load_from_secret_manager();
    /// let dispatcher = MyServer.into_server().with_state_key(key).build();
    /// ```
    #[must_use]
    pub fn with_state_key(self, key: [u8; 32]) -> Self {
        self.with_state_keys(key, [])
    }

    /// Seal MRTR `requestState` with `current`, and also accept states sealed
    /// with any of `previous`: zero-downtime key rotation.
    ///
    /// `requestState` is the opaque blob a handler hands back with an
    /// elicitation so the re-issued request can resume where it left off. It
    /// is sealed (XChaCha20-Poly1305), so the client can neither read what the
    /// handler stored nor alter it. The default key is minted per dispatcher,
    /// which is correct for a single process, but it means **a state minted by
    /// one replica cannot be redeemed by another, and a restart invalidates
    /// every outstanding one**. Set shared keys on every replica when you run
    /// more than one, or when in-flight elicitations must survive a rolling
    /// deploy. This is the key counterpart to
    /// [`with_session_backend`](Self::with_session_backend) /
    /// [`with_task_backend`](Self::with_task_backend): sharing the *stores*
    /// across replicas does not help if the *key* is still per-process.
    ///
    /// To rotate: deploy the new key as `previous` everywhere, then as
    /// `current` with the old one as `previous`, then drop the old one once
    /// its states have expired ([`request_state_ttl`](Self::request_state_ttl),
    /// ten minutes by default). Each state names the key that sealed it, so no
    /// state in flight fails along the way.
    ///
    /// The keys are secrets: 32 bytes from a CSPRNG each, held in your secret
    /// manager, never derived from a passphrase or hard-coded. Anyone holding
    /// one can mint states this server will accept, though a forged state
    /// still can't cross principals or requests, since both are bound into it.
    #[must_use]
    pub fn with_state_keys(
        self,
        current: [u8; 32],
        previous: impl IntoIterator<Item = [u8; 32]>,
    ) -> Self {
        let previous: Vec<[u8; 32]> = previous.into_iter().collect();
        self.setting("with_state_keys", move |d| {
            d.with_state_keys(current, previous)
        })
    }

    /// Cap the serialized MRTR `requestState` at `bytes` (default 256 KiB).
    ///
    /// Every answer a multi-round handler has collected — elicitation content,
    /// sampling replies — rides the state into the next round, so a flow with
    /// long sampling answers may need more. Going over is a `-32603` naming
    /// this setting. The state is signed, not encrypted: the client can read
    /// it.
    #[must_use]
    pub fn request_state_limit(self, bytes: usize) -> Self {
        self.setting("request_state_limit", move |d| d.request_state_limit(bytes))
    }

    /// How long an issued MRTR `requestState` stays redeemable (default ten
    /// minutes): the bound on how long a client may take to answer, and on how
    /// long a captured state could be replayed.
    #[must_use]
    pub fn request_state_ttl(self, ttl: std::time::Duration) -> Self {
        self.setting("request_state_ttl", move |d| d.request_state_ttl(ttl))
    }

    /// The server and its capability registrations, dropping everything else.
    ///
    /// For [`Composite::mount`](crate::Composite::mount), which wants exactly
    /// the pair `(server, what it implements)` and configures the one dispatcher
    /// itself. Paired with [`dispatcher_setting`](Self::dispatcher_setting) so
    /// dropping the rest is a checked error rather than a silent loss.
    pub(crate) fn into_parts(self) -> (S, MethodRouter<S>) {
        (self.server, self.router)
    }

    /// The name of a dispatcher-level setting made on this builder, if any:
    /// a builder about to be mounted (where the dispatcher belongs to the
    /// composite) must carry none.
    pub(crate) fn dispatcher_setting(&self) -> Option<&'static str> {
        self.settings.first().map(|(name, _)| *name)
    }

    /// Record a dispatcher setting made by the builder method `name`.
    fn setting(
        mut self,
        name: &'static str,
        apply: impl FnOnce(VersionDispatcher<S>) -> VersionDispatcher<S> + Send + 'static,
    ) -> Self {
        self.settings.push((name, Box::new(apply)));
        self
    }

    /// Wrap the server in RPC middleware: `layer` is a [`tower::Layer`] over
    /// the `Service<McpRequest>` seam. Chain more with
    /// [`Server::layer`](crate::Server::layer); the first added is outermost.
    #[must_use]
    pub fn layer<T>(
        self,
        layer: T,
    ) -> crate::Server<S, tower::layer::util::Stack<T, tower::layer::util::Identity>> {
        crate::Server::new(self.build()).layer(layer)
    }

    /// Build the server and serve it on `target`: `stdio()` or any other
    /// [`Transport`](turbomcp_service::Transport) for one connection, or a
    /// network listener such as `turbomcp-transport-http`'s `Http`. See
    /// [`Server::serve`](crate::Server::serve).
    ///
    /// # Errors
    /// Whatever the transport fails with.
    pub async fn serve<T: turbomcp_service::Serve>(
        self,
        target: T,
    ) -> Result<(), turbomcp_service::ProtocolError> {
        crate::Server::new(self.build()).serve(target).await
    }

    /// Finish: produce the `tower::Service<McpRequest>` for this server.
    #[must_use]
    pub fn build(self) -> VersionDispatcher<S> {
        self.settings.into_iter().fold(
            VersionDispatcher::new(self.server, self.router),
            |d, (_, apply)| apply(d),
        )
    }
}

/// Blanket entry point so any [`McpServerCore`] gets `into_server()`.
pub trait IntoServerBuilder: McpServerCore + Sized {
    /// Begin building a server from the capabilities it registers
    /// ([`McpServerCore::register`]); chain `with_*` to add more.
    fn into_server(self) -> ServerBuilder<Self> {
        ServerBuilder::new(self)
    }
}

impl<S: McpServerCore> IntoServerBuilder for S {}

/// Two session observers, told in the order they were added.
pub(crate) struct SessionObservers(pub(crate) [Arc<dyn turbomcp_service::SessionObserver>; 2]);

impl turbomcp_service::SessionObserver for SessionObservers {
    fn session_ended(&self, session: &turbomcp_service::EndedSession<'_>) {
        for observer in &self.0 {
            observer.session_ended(session);
        }
    }
}
