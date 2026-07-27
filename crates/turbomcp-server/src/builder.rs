//! [`ServerBuilder`]: assemble a server value and the capabilities it implements
//! into a [`VersionDispatcher`] ready to hand to a transport.
//!
//! The builder is deliberately transport- and codec-agnostic: it produces the
//! `tower::Service<JsonRpcMessage>` and nothing more. Codec selection, RPC
//! middleware stacks (`with_rpc_middleware`), and extensions (`with_extension`)
//! attach at the transport/facade layer and land in Phases 4/8 — adding them
//! here now would be infrastructure with no consumer.
//!
//! Two entry points:
//! - [`ServerBuilder::new`] starts with an empty router; chain `with_tools()`
//!   etc. to register the capabilities the server implements.
//! - [`IntoServerBuilder::into_server`] (blanket-implemented for every
//!   [`McpServerCore`]) gives the same empty-router builder as a method, so
//!   `my_server.into_server()` works. The `#[server]` macro emits an *inherent*
//!   `into_server` on the user's type that pre-registers exactly the capabilities
//!   it found (inherent methods shadow the trait method, so there's no clash).

use std::sync::Arc;

use crate::dispatcher::{CachePolicies, VersionDispatcher};
use crate::extension::Extension;
use crate::router::MethodRouter;
use crate::session::SessionBackend;
use crate::tasks::TaskBackend;
use crate::traits::{McpServerCore, WithCompletions, WithPrompts, WithResources, WithTools};

/// Assembles a server and its [`MethodRouter`] into a [`VersionDispatcher`].
pub struct ServerBuilder<S> {
    server: S,
    router: MethodRouter<S>,
    tasks: bool,
    strict_elicitation_keys: bool,
    session_idle_timeout: Option<std::time::Duration>,
    session_backend: Option<Arc<dyn SessionBackend>>,
    task_backend: Option<Arc<dyn TaskBackend>>,
    extensions: Vec<Arc<dyn Extension>>,
    cache: Option<CachePolicies>,
    state_key: Option<[u8; 32]>,
}

impl<S: McpServerCore> ServerBuilder<S> {
    /// Start from `server` with no capabilities registered.
    #[must_use]
    pub fn new(server: S) -> Self {
        Self {
            server,
            router: MethodRouter::new(),
            tasks: false,
            strict_elicitation_keys: false,
            session_idle_timeout: None,
            session_backend: None,
            task_backend: None,
            extensions: Vec::new(),
            cache: None,
            state_key: None,
        }
    }

    /// Start from a server and a pre-built router (what the `#[server]` macro
    /// emits once it has registered every discovered capability).
    #[must_use]
    pub fn from_parts(server: S, router: MethodRouter<S>) -> Self {
        Self {
            server,
            router,
            tasks: false,
            strict_elicitation_keys: false,
            session_idle_timeout: None,
            session_backend: None,
            task_backend: None,
            extensions: Vec::new(),
            cache: None,
            state_key: None,
        }
    }

    /// Set the cache defaults (SEP-2549) advertised on draft cacheable
    /// results: a bare [`CachePolicy`](turbomcp_protocol::neutral::CachePolicy)
    /// for a uniform policy, or a [`CachePolicies`] for per-capability
    /// control. See [`VersionDispatcher::with_cache_policy`].
    #[must_use]
    pub fn cache_policy(mut self, cache: impl Into<CachePolicies>) -> Self {
        self.cache = Some(cache.into());
        self
    }

    /// Evict a legacy (`2025-11-25`) session not seen within `timeout`, tearing
    /// down its subscription routes (see
    /// [`VersionDispatcher::with_session_idle_timeout`]). Without it, sessions
    /// are bounded only by the store's LRU capacity.
    #[must_use]
    pub fn session_idle_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.session_idle_timeout = Some(timeout);
        self
    }

    /// Enable core Tasks (`2025-11-25`): task-augmented `tools/call` plus
    /// `tasks/list|get|cancel|result`. See
    /// [`VersionDispatcher::with_task_support`]. Meaningful only alongside a
    /// registered tools capability.
    #[must_use]
    pub fn with_tasks(mut self) -> Self {
        self.tasks = true;
        self
    }

    /// Enable core Tasks backed by a custom [`TaskBackend`] instead of the
    /// bundled in-memory store — the seam for external task storage. See
    /// [`VersionDispatcher::with_task_backend`]. Implies
    /// [`with_tasks`](Self::with_tasks).
    #[must_use]
    pub fn with_task_backend(mut self, backend: Arc<dyn TaskBackend>) -> Self {
        self.task_backend = Some(backend);
        self
    }

    /// Store legacy (`2025-11-25`) session state in a custom
    /// [`SessionBackend`] instead of the bundled in-memory store — the seam
    /// for external session storage (e.g. Redis), so multiple instances can
    /// serve the same session. When set,
    /// [`session_idle_timeout`](Self::session_idle_timeout) is ignored
    /// (eviction policy belongs to the backend). See
    /// [`VersionDispatcher::with_session_backend`].
    #[must_use]
    pub fn with_session_backend(mut self, backend: Arc<dyn SessionBackend>) -> Self {
        self.session_backend = Some(backend);
        self
    }

    /// Opt in to strict elicitation keys: reusing an `elicit` key with a
    /// different request shape in one handler execution is an error, not a
    /// warning. See [`VersionDispatcher::strict_elicitation_keys`].
    #[must_use]
    pub fn strict_elicitation_keys(mut self) -> Self {
        self.strict_elicitation_keys = true;
        self
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
    pub fn with_extension(mut self, extension: Arc<dyn Extension>) -> Self {
        self.extensions.push(extension);
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

    /// Sign MRTR `requestState` with a key you supply, rather than the
    /// per-process random secret used by default.
    ///
    /// `requestState` is the opaque, HMAC-signed blob a handler hands back with
    /// an elicitation so the re-issued request can resume where it left off.
    /// The default key is minted per dispatcher, which is correct for a single
    /// process — but it means **a state minted by one replica cannot be
    /// redeemed by another, and a restart invalidates every outstanding one**.
    /// Set a shared key on every replica when you run more than one, or when
    /// in-flight elicitations must survive a rolling deploy. This is the
    /// signing-key counterpart to
    /// [`with_session_backend`](Self::with_session_backend) /
    /// [`with_task_backend`](Self::with_task_backend): sharing the *stores*
    /// across replicas does not help if the *signature* is still per-process.
    ///
    /// The key is a MAC secret, so treat it like one: 32 bytes from a CSPRNG,
    /// held in your secret manager, never derived from a passphrase or
    /// hard-coded. Anyone holding it can mint states this server will accept —
    /// though a forged state still can't cross principals or methods, since
    /// both are bound into the signed payload. Rotating the key invalidates
    /// outstanding states; handlers see the re-issued request fail
    /// verification, which is the same path a tampered state takes.
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
    pub fn with_state_key(mut self, key: [u8; 32]) -> Self {
        self.state_key = Some(key);
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

    /// The name of a dispatcher-level setting made on this builder, if any.
    ///
    /// Every field here besides `server`/`router` configures the *dispatcher*,
    /// not the server, so a builder that is about to be mounted (where the
    /// dispatcher belongs to the composite) must carry none of them. Listed
    /// explicitly rather than by a `..` catch-all: a new setting should make
    /// this fail to compile until someone decides which side it belongs on.
    pub(crate) fn dispatcher_setting(&self) -> Option<&'static str> {
        let Self {
            server: _,
            router: _,
            tasks,
            strict_elicitation_keys,
            session_idle_timeout,
            session_backend,
            task_backend,
            extensions,
            cache,
            state_key,
        } = self;
        if *tasks {
            return Some("with_tasks");
        }
        if *strict_elicitation_keys {
            return Some("strict_elicitation_keys");
        }
        if session_idle_timeout.is_some() {
            return Some("session_idle_timeout");
        }
        if session_backend.is_some() {
            return Some("with_session_backend");
        }
        if task_backend.is_some() {
            return Some("with_task_backend");
        }
        if !extensions.is_empty() {
            return Some("with_extension");
        }
        if cache.is_some() {
            return Some("cache_policy");
        }
        if state_key.is_some() {
            return Some("with_state_key");
        }
        None
    }

    /// Finish: produce the `tower::Service<JsonRpcMessage>` for this server.
    #[must_use]
    pub fn build(self) -> VersionDispatcher<S> {
        let mut dispatcher = VersionDispatcher::new(self.server, self.router);
        if let Some(backend) = self.task_backend {
            dispatcher = dispatcher.with_task_backend(backend);
        } else if self.tasks {
            dispatcher = dispatcher.with_task_support();
        }
        if self.strict_elicitation_keys {
            dispatcher = dispatcher.strict_elicitation_keys();
        }
        if let Some(backend) = self.session_backend {
            dispatcher = dispatcher.with_session_backend(backend);
        } else if let Some(timeout) = self.session_idle_timeout {
            dispatcher = dispatcher.with_session_idle_timeout(timeout);
        }
        for extension in self.extensions {
            dispatcher = dispatcher.with_extension(extension);
        }
        if let Some(cache) = self.cache {
            dispatcher = dispatcher.with_cache_policy(cache);
        }
        if let Some(key) = self.state_key {
            dispatcher = dispatcher.with_state_key(key);
        }
        dispatcher
    }
}

/// Blanket entry point so any [`McpServerCore`] gets `into_server()`. The macro
/// shadows this with an inherent method that pre-registers capabilities.
pub trait IntoServerBuilder: McpServerCore + Sized {
    /// Begin building a server (empty router; chain `with_*` to register).
    fn into_server(self) -> ServerBuilder<Self> {
        ServerBuilder::new(self)
    }
}

impl<S: McpServerCore> IntoServerBuilder for S {}
