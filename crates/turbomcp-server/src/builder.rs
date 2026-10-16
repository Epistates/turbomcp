//! [`ServerBuilder`]: assemble a server value and the capabilities it implements
//! into a [`VersionDispatcher`] ready to hand to a transport.
//!
//! The builder is transport- and codec-agnostic: it produces the
//! `tower::Service<McpRequest>`, and [`layer`](ServerBuilder::layer) /
//! [`serve`](ServerBuilder::serve) hand that to RPC middleware and a
//! transport.
//!
//! Its settings are a [`ServerConfig`]: each `with_*` setting method writes
//! the [`ServerConfig`] method of the same name, documented there, and
//! [`build`](ServerBuilder::build) makes the dispatcher from it. A whole
//! config can be handed over at once with
//! [`with_config`](ServerBuilder::with_config).
//!
//! [`ServerBuilder::new`] (or [`IntoServerBuilder::into_server`], the same
//! thing as a method) starts from the capabilities the server registers
//! ([`McpServerCore::register`], which `#[server]` writes from its markers).
//! Chain `with_tools()` etc. to add more.

use std::sync::Arc;

use crate::config::ServerConfig;
use crate::dispatcher::{CachePolicies, VersionDispatcher};
use crate::extension::Extension;
use crate::router::MethodRouter;
use crate::session::SessionBackend;
use crate::tasks::TaskBackend;
use crate::traits::{McpServerCore, WithCompletions, WithPrompts, WithResources, WithTools};

/// Unbounded in `S`, and names the settings made rather than the trait
/// objects behind them.
impl<S> core::fmt::Debug for ServerBuilder<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("router", &self.router)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// Assembles a server and its [`MethodRouter`] into a [`VersionDispatcher`].
pub struct ServerBuilder<S> {
    server: S,
    router: MethodRouter<S>,
    config: ServerConfig,
}

/// A builder method that sets the [`ServerConfig`] setting of the same name.
macro_rules! config_setting {
    ($(#[$doc:meta])* $name:ident($($arg:ident: $ty:ty),*)) => {
        $(#[$doc])*
        #[doc = concat!("See [`ServerConfig::", stringify!($name), "`].")]
        #[must_use]
        pub fn $name(mut self, $($arg: $ty),*) -> Self {
            self.config = self.config.$name($($arg),*);
            self
        }
    };
}

impl<S: McpServerCore> ServerBuilder<S> {
    /// Start from `server` with the capabilities it registers
    /// ([`McpServerCore::register`]).
    #[must_use]
    pub fn new(server: S) -> Self {
        Self::from_parts(server, S::register(MethodRouter::new()))
    }

    /// Start from a server and a router built for it elsewhere: a server
    /// whose capabilities are known only at run time (a gateway's upstream,
    /// say) rather than from [`McpServerCore::register`].
    #[must_use]
    pub fn from_parts(server: S, router: MethodRouter<S>) -> Self {
        Self {
            server,
            router,
            config: ServerConfig::default(),
        }
    }

    /// Replace every setting with `config`'s: a configuration built once
    /// (from a deployment's environment, say) and shared by several servers.
    /// Settings made after this apply on top of it.
    #[must_use]
    pub fn with_config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    config_setting!(
        /// Cache defaults (SEP-2549) for `2026-07-28` cacheable results.
        with_cache_policy(cache: impl Into<CachePolicies>)
    );
    config_setting!(
        /// Expire an idle stateful session after `timeout`.
        with_session_idle_timeout(timeout: std::time::Duration)
    );
    config_setting!(
        /// Keep stateful session state in a backend of your own.
        with_session_backend(backend: Arc<dyn SessionBackend>)
    );
    config_setting!(
        /// Turn on `2025-11-25` core Tasks.
        with_tasks()
    );
    config_setting!(
        /// Core Tasks backed by a store of your own.
        with_task_backend(backend: Arc<dyn TaskBackend>)
    );
    config_setting!(
        /// The task support of a tool that declares none.
        with_undeclared_task_support(support: turbomcp_protocol::neutral::TaskSupport)
    );
    config_setting!(
        /// Make a reused elicitation key with another shape an error.
        with_strict_elicitation_keys()
    );
    config_setting!(
        /// Register an extension (SEP-2133).
        with_extension(extension: Arc<dyn Extension>)
    );
    config_setting!(
        /// Install a visibility policy (progressive disclosure).
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
        with_visibility(policy: Arc<dyn crate::VisibilityPolicy>)
    );
    config_setting!(
        /// Hear when a client's roots change.
        with_roots_changed_handler(
            handler: impl Fn(&turbomcp_core::RequestContext) + Send + Sync + 'static
        )
    );
    config_setting!(
        /// Be told when each stateful session starts and ends.
        with_session_observer(observer: Arc<dyn turbomcp_service::SessionObserver>)
    );
    config_setting!(
        /// Carry change notifications between replicas.
        with_notification_bus(bus: Arc<dyn crate::NotificationBus>)
    );
    config_setting!(
        /// Bound how long a call may run.
        with_call_timeout(limit: std::time::Duration)
    );
    config_setting!(
        /// What a client's cancellation does to the handler.
        with_cancellation(cancellation: crate::Cancellation)
    );
    config_setting!(
        /// How many compiled tool schemas to keep.
        with_schema_cache_capacity(capacity: u64)
    );
    config_setting!(
        /// Send clients a reference instead of an internal error's text.
        with_masked_internal_errors()
    );
    config_setting!(
        /// Seal MRTR `requestState` with a key of your own.
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
        with_state_key(key: [u8; 32])
    );
    config_setting!(
        /// Seal MRTR `requestState` with rotating keys.
        with_state_keys(current: [u8; 32], previous: impl IntoIterator<Item = [u8; 32]>)
    );
    config_setting!(
        /// Cap the size of MRTR `requestState`.
        with_request_state_limit(bytes: usize)
    );
    config_setting!(
        /// How long an MRTR `requestState` stays redeemable.
        with_request_state_ttl(ttl: std::time::Duration)
    );
    config_setting!(
        /// How long an inline request to a `2025-*` client waits.
        with_inline_request_timeout(timeout: std::time::Duration)
    );

    /// Enable the `logging` capability: handlers gain a live `ctx.log` when
    /// the client opts in (`logging/setLevel` per session on `2025-*`;
    /// per-request `_meta` `io.modelcontextprotocol/logLevel` on
    /// `2026-07-28`, where SEP-2577 deprecates the feature: prefer
    /// `stderr`/OpenTelemetry for new servers). Without the opt-in, `ctx.log`
    /// drops everything, per the logging spec's MUST NOT.
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.router = self.router.with_logging();
        self
    }

    /// Run every operation through `interceptor`, inside any added before
    /// it: typed middleware over the neutral request and result, on every
    /// revision and every path a call takes. See [`crate::intercept`].
    #[must_use]
    pub fn with_interceptor(mut self, interceptor: Arc<dyn crate::Interceptor>) -> Self {
        self.router = self.router.with_interceptor(interceptor);
        self
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

    /// The server and its capability registrations, dropping everything else.
    ///
    /// For [`Composite::mount`](crate::Composite::mount), which wants exactly
    /// the pair `(server, what it implements)` and configures the one dispatcher
    /// itself. Paired with [`dispatcher_setting`](Self::dispatcher_setting) so
    /// dropping the rest is a checked error rather than a silent loss.
    pub(crate) fn into_parts(self) -> (S, MethodRouter<S>) {
        (self.server, self.router)
    }

    /// The name of a setting made on this builder, if any: a builder about to
    /// be mounted (where the dispatcher belongs to the composite) must carry
    /// none.
    pub(crate) fn dispatcher_setting(&self) -> Option<&'static str> {
        self.config.first_setting()
    }

    /// Wrap the server in RPC middleware: `layer` is a [`tower::Layer`] over
    /// the `Service<McpRequest>` seam. Chain more with
    /// [`Server::layer`](crate::Server::layer); the first added is outermost.
    #[must_use]
    pub fn layer<T>(
        self,
        layer: T,
    ) -> crate::Server<S, tower::layer::util::Stack<T, tower::layer::util::Identity>> {
        self.into_runtime().layer(layer)
    }

    /// Build it into a [`Server`](crate::Server) without serving it yet: take
    /// its [`notifier`](crate::Server::notifier) to announce list and resource
    /// changes from outside any request, then [`serve`](crate::Server::serve)
    /// it.
    #[must_use]
    pub fn into_runtime(self) -> crate::Server<S> {
        crate::Server::new(self.build())
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
        self.into_runtime().serve(target).await
    }

    /// Finish: produce the `tower::Service<McpRequest>` for this server.
    #[must_use]
    pub fn build(self) -> VersionDispatcher<S> {
        VersionDispatcher::with_config(self.server, self.router, self.config)
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

/// Several session observers, told in the order they were added.
pub(crate) struct SessionObservers(pub(crate) Vec<Arc<dyn turbomcp_service::SessionObserver>>);

impl turbomcp_service::SessionObserver for SessionObservers {
    fn session_started(&self, session: &turbomcp_service::StartedSession<'_>) {
        for observer in &self.0 {
            observer.session_started(session);
        }
    }

    fn session_ended(&self, session: &turbomcp_service::EndedSession<'_>) {
        for observer in &self.0 {
            observer.session_ended(session);
        }
    }
}
