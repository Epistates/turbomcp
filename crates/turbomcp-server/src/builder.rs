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
