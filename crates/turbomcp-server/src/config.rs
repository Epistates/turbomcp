//! [`ServerConfig`]: every setting a server takes, in one place.
//!
//! [`ServerBuilder`](crate::ServerBuilder)'s methods of the same names write
//! into one, and [`VersionDispatcher::with_config`](crate::VersionDispatcher::with_config)
//! builds the dispatcher from it, so a setting means the same thing however
//! it is made. Build one on its own to share it (`ServerBuilder::with_config`):
//! a deployment's settings read once from its environment, applied to every
//! server it runs.

use std::sync::Arc;
use std::time::Duration;

use turbomcp_protocol::neutral::TaskSupport;

use crate::dispatcher::{CachePolicies, Cancellation, RootsChangedHandler};
use crate::extension::Extension;
use crate::session::SessionBackend;
use crate::tasks::TaskBackend;

/// How long an inline (`2025-*`) request to the client waits for its answer,
/// by default.
pub(crate) const DEFAULT_INLINE_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Every setting a server takes. Start from [`ServerConfig::new`] (the
/// defaults) and set what differs with the `with_*` methods.
#[derive(Clone)]
#[non_exhaustive]
pub struct ServerConfig {
    pub(crate) cache: CachePolicies,
    pub(crate) session_idle_timeout: Option<Duration>,
    pub(crate) session_backend: Option<Arc<dyn SessionBackend>>,
    pub(crate) tasks: bool,
    pub(crate) task_backend: Option<Arc<dyn TaskBackend>>,
    pub(crate) undeclared_tasks: TaskSupport,
    pub(crate) strict_elicitation_keys: bool,
    pub(crate) extensions: Vec<Arc<dyn Extension>>,
    pub(crate) visibility: Option<Arc<dyn crate::VisibilityPolicy>>,
    pub(crate) roots_changed: Option<Arc<RootsChangedHandler>>,
    pub(crate) session_observers: Vec<Arc<dyn turbomcp_service::SessionObserver>>,
    pub(crate) bus: Option<Arc<dyn crate::NotificationBus>>,
    pub(crate) call_timeout: Option<Duration>,
    pub(crate) cancellation: Cancellation,
    pub(crate) schema_cache_capacity: Option<u64>,
    pub(crate) mask_internal_errors: bool,
    pub(crate) state_keys: Option<([u8; 32], Vec<[u8; 32]>)>,
    pub(crate) request_state_limit: Option<usize>,
    pub(crate) request_state_ttl: Option<Duration>,
    pub(crate) inline_request_timeout: Duration,
    /// The settings made, by name, in order: a builder about to be mounted
    /// (whose dispatcher is the composite's) must have made none.
    set: Vec<&'static str>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            cache: CachePolicies::default(),
            session_idle_timeout: None,
            session_backend: None,
            tasks: false,
            task_backend: None,
            undeclared_tasks: TaskSupport::Optional,
            strict_elicitation_keys: false,
            extensions: Vec::new(),
            visibility: None,
            roots_changed: None,
            session_observers: Vec::new(),
            bus: None,
            call_timeout: None,
            cancellation: Cancellation::default(),
            schema_cache_capacity: None,
            mask_internal_errors: false,
            state_keys: None,
            request_state_limit: None,
            request_state_ttl: None,
            inline_request_timeout: DEFAULT_INLINE_REQUEST_TIMEOUT,
            set: Vec::new(),
        }
    }
}

/// Names the settings made, never their values: the state keys are secrets,
/// and the seams are trait objects.
impl core::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("set", &self.set)
            .finish_non_exhaustive()
    }
}

impl ServerConfig {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The first setting made, if any.
    pub(crate) fn first_setting(&self) -> Option<&'static str> {
        self.set.first().copied()
    }

    fn mark(mut self, name: &'static str) -> Self {
        self.set.push(name);
        self
    }

    /// The cache defaults (SEP-2549) advertised on `2026-07-28` cacheable
    /// results (`server/discover`, the four `*/list`s, `resources/read`):
    /// a [`CachePolicy`](turbomcp_protocol::neutral::CachePolicy) for all of
    /// them, or [`CachePolicies`] per capability. A policy a handler sets on
    /// its result wins. Without this, every cacheable result is immediately
    /// stale (`ttlMs: 0`, `cacheScope: "private"`).
    #[must_use]
    pub fn with_cache_policy(mut self, cache: impl Into<CachePolicies>) -> Self {
        self.cache = cache.into();
        self.mark("with_cache_policy")
    }

    /// Expire a stateful (`2025-*`) session not used within `timeout`
    /// (default [`SessionStore::DEFAULT_IDLE_TIMEOUT`], an hour), tearing
    /// down its subscription routes and `GET` stream. Ignored beside
    /// [`with_session_backend`](Self::with_session_backend): a backend keeps
    /// its own eviction policy.
    ///
    /// [`SessionStore::DEFAULT_IDLE_TIMEOUT`]: crate::SessionStore::DEFAULT_IDLE_TIMEOUT
    #[must_use]
    pub fn with_session_idle_timeout(mut self, timeout: Duration) -> Self {
        self.session_idle_timeout = Some(timeout);
        self.mark("with_session_idle_timeout")
    }

    /// Keep stateful (`2025-*`) session state in `backend` instead of the
    /// bundled in-memory store: the seam for external session storage
    /// (Redis, say), so sessions outlive a restart and any replica a session
    /// is routed to can find it. The `2025-*` wire still needs sticky
    /// routing on `Mcp-Session-Id` (see [`SessionBackend`]).
    #[must_use]
    pub fn with_session_backend(mut self, backend: Arc<dyn SessionBackend>) -> Self {
        self.session_backend = Some(backend);
        self.mark("with_session_backend")
    }

    /// Turn on `2025-11-25` core Tasks: task-augmented `tools/call` and
    /// `tasks/list|get|cancel|result`, advertised by the `tasks`
    /// capability. Meaningful with a tools capability. Task-augmented calls
    /// run on spawned tasks, so the server must run inside a tokio runtime
    /// (every bundled transport does).
    #[must_use]
    pub fn with_tasks(mut self) -> Self {
        self.tasks = true;
        self.mark("with_tasks")
    }

    /// Core Tasks backed by `backend` instead of the bundled in-memory store:
    /// the seam for external task storage. Implies
    /// [`with_tasks`](Self::with_tasks). The `2026-07-28` Tasks extension can
    /// be given the same backend (`TasksExtension::backend`), so one registry
    /// fronts both wires.
    #[must_use]
    pub fn with_task_backend(mut self, backend: Arc<dyn TaskBackend>) -> Self {
        self.task_backend = Some(backend);
        self.tasks = true;
        self.mark("with_task_backend")
    }

    /// The `2025-11-25` `execution.taskSupport` of a tool that declares none
    /// once Tasks are on (default `optional`: every tool may run as a task).
    /// [`TaskSupport::Forbidden`] makes tasks opt-in per tool
    /// (`#[tool(task)]`), the spec's own default.
    #[must_use]
    pub fn with_undeclared_task_support(mut self, support: TaskSupport) -> Self {
        self.undeclared_tasks = support;
        self.mark("with_undeclared_task_support")
    }

    /// Make reusing an `elicit` key with a differently shaped request, in one
    /// handler execution, an error rather than a warning: an idempotency
    /// lint for multi-round handlers.
    #[must_use]
    pub fn with_strict_elicitation_keys(mut self) -> Self {
        self.strict_elicitation_keys = true;
        self.mark("with_strict_elicitation_keys")
    }

    /// Register an [`Extension`] (SEP-2133): advertised under
    /// `capabilities.extensions[id]` to clients on the revisions it speaks,
    /// in `initialize` or `server/discover`, and owning its methods there.
    /// The reference extension is the Tasks extension (`turbomcp-ext-tasks`).
    ///
    /// # Panics
    /// When it collides: an id already registered, a method the core protocol
    /// defines on a revision it speaks, or a method another extension claims
    /// on a revision both speak. Registration is configuration, so a conflict
    /// is a bug to fix rather than a request to answer.
    #[must_use]
    pub fn with_extension(mut self, extension: Arc<dyn Extension>) -> Self {
        crate::extension::check_registration(&self.extensions, extension.as_ref());
        self.extensions.push(extension);
        self.mark("with_extension")
    }

    /// Install a [`VisibilityPolicy`](crate::VisibilityPolicy), progressive
    /// disclosure: what it hides is dropped from every list *and* answered as
    /// though it did not exist, since filtering a list without refusing the
    /// call would be theatre. See [the module docs](crate::visibility).
    #[must_use]
    pub fn with_visibility(mut self, policy: Arc<dyn crate::VisibilityPolicy>) -> Self {
        self.visibility = Some(policy);
        self.mark("with_visibility")
    }

    /// Run `handler` when a client says its roots changed
    /// (`notifications/roots/list_changed`), the one list change that travels
    /// client to server. Its job is to invalidate, not to re-read: a
    /// notification carries no [`ClientHandle`](crate::ClientHandle), so the
    /// roots are fetched again from the next request that needs them. It runs
    /// on the dispatch path, so keep it cheap. The
    /// [`RequestContext`](turbomcp_core::RequestContext) says whose roots
    /// moved.
    #[must_use]
    pub fn with_roots_changed_handler(
        mut self,
        handler: impl Fn(&turbomcp_core::RequestContext) + Send + Sync + 'static,
    ) -> Self {
        self.roots_changed = Some(Arc::new(handler));
        self.mark("with_roots_changed_handler")
    }

    /// Tell `observer` when each stateful session starts and ends (and how
    /// long it lived): `turbomcp-telemetry`'s `MetricsLayer` records
    /// `mcp.server.session.duration` from it, and a per-session gateway
    /// upstream closes on it. Observers accumulate.
    #[must_use]
    pub fn with_session_observer(
        mut self,
        observer: Arc<dyn turbomcp_service::SessionObserver>,
    ) -> Self {
        self.session_observers.push(observer);
        self.mark("with_session_observer")
    }

    /// Carry change notifications between replicas over `bus`: the
    /// [`ServerNotifier`](crate::ServerNotifier) publishes each change to it,
    /// and every replica delivers what it carries to the subscriptions it
    /// holds. See [the `bus` module](crate::bus).
    #[must_use]
    pub fn with_notification_bus(mut self, bus: Arc<dyn crate::NotificationBus>) -> Self {
        self.bus = Some(bus);
        self.mark("with_notification_bus")
    }

    /// Cancel a call still running after `limit` and answer it with a
    /// timeout error, so a hung handler can't hold an admission slot for
    /// ever; its cancellation fires (see
    /// [`with_cancellation`](Self::with_cancellation)). Per request: a
    /// multi-round (`2026-07-28`) call gets `limit` per round, and a
    /// task-augmented call to return its task, not for the task's work
    /// (bounded by its TTL). Default: no limit.
    #[must_use]
    pub fn with_call_timeout(mut self, limit: Duration) -> Self {
        self.call_timeout = Some(limit);
        self.mark("with_call_timeout")
    }

    /// What a client's cancellation does to the handler serving the request.
    /// [`Cancellation::Cooperative`] (the default) fires the request's token
    /// and lets the handler finish its own way, its result discarded;
    /// [`Cancellation::Abort`] drops it at its next `.await`.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = cancellation;
        self.mark("with_cancellation")
    }

    /// How many compiled tool schemas to keep (default 256). Every
    /// `tools/call` validates its arguments, and a structured result its
    /// output, against a validator compiled once per distinct schema; a
    /// server or gateway offering more distinct schemas than this recompiles
    /// on churn.
    #[must_use]
    pub fn with_schema_cache_capacity(mut self, capacity: u64) -> Self {
        self.schema_cache_capacity = Some(capacity);
        self.mark("with_schema_cache_capacity")
    }

    /// Send clients a reference instead of an internal error's text.
    ///
    /// An internal error is written for the operator, and often wraps what a
    /// client should not see: a SQL statement, a hostname, a connection
    /// string. With this set, every internal error the server answers (a
    /// JSON-RPC `-32603`, or a tool's `isError` text) reads
    /// `internal error (ref: <id>)`, and the original is logged through
    /// `tracing` at `error` under the same `error_ref`. Errors meant for the
    /// model (`McpError::tool_execution_failed`, invalid arguments) are
    /// unchanged. Recommended in production.
    #[must_use]
    pub fn with_masked_internal_errors(mut self) -> Self {
        self.mask_internal_errors = true;
        self.mark("with_masked_internal_errors")
    }

    /// Seal MRTR `requestState` with `key` rather than a per-process random
    /// secret: [`with_state_keys`](Self::with_state_keys)`(key, [])`.
    #[must_use]
    pub fn with_state_key(self, key: [u8; 32]) -> Self {
        self.with_state_keys(key, [])
    }

    /// Seal MRTR `requestState` with `current`, and accept states sealed with
    /// any of `previous`: zero-downtime key rotation.
    ///
    /// `requestState` is the opaque blob a handler hands back with an
    /// elicitation so the retried request resumes where it left off. It is
    /// sealed (XChaCha20-Poly1305), so the client can neither read nor alter
    /// it. The default key is minted per server, which is right for one
    /// process, but **a state minted by one replica can't be redeemed by
    /// another, and a restart invalidates every outstanding one**. Set shared
    /// keys on every replica when you run more than one, or when in-flight
    /// elicitations must survive a rolling deploy: sharing the session and
    /// task stores doesn't help while the key is per process.
    ///
    /// To rotate: deploy the new key as `previous` everywhere, then as
    /// `current` with the old one as `previous`, then drop the old one once
    /// its states have expired ([`with_request_state_ttl`](Self::with_request_state_ttl),
    /// ten minutes by default). Each state names the key that sealed it.
    ///
    /// The keys are secrets: 32 bytes from a CSPRNG each, in your secret
    /// manager, never derived from a passphrase or written in code.
    #[must_use]
    pub fn with_state_keys(
        mut self,
        current: [u8; 32],
        previous: impl IntoIterator<Item = [u8; 32]>,
    ) -> Self {
        self.state_keys = Some((current, previous.into_iter().collect()));
        self.mark("with_state_keys")
    }

    /// Cap the serialized MRTR `requestState` at `bytes` (default 256 KiB).
    /// Every answer a multi-round handler collected rides the state into the
    /// next round, so a flow with long sampling answers may need more; going
    /// over is an internal error naming this setting.
    #[must_use]
    pub fn with_request_state_limit(mut self, bytes: usize) -> Self {
        self.request_state_limit = Some(bytes);
        self.mark("with_request_state_limit")
    }

    /// How long an issued MRTR `requestState` stays redeemable (default ten
    /// minutes): the bound on how long a client may take to answer, and on
    /// how long a captured state could be replayed.
    #[must_use]
    pub fn with_request_state_ttl(mut self, ttl: Duration) -> Self {
        self.request_state_ttl = Some(ttl);
        self.mark("with_request_state_ttl")
    }

    /// How long a handler's inline request to a `2025-*` client
    /// (elicitation, sampling, roots, sent as a request on the session)
    /// waits for the answer before failing with a timeout (default two
    /// minutes). An elicitation waits on a person: raise it for forms that
    /// take a while to fill in. (On `2026-07-28` the client answers by
    /// retrying the call, and `with_request_state_ttl` bounds that.)
    #[must_use]
    pub fn with_inline_request_timeout(mut self, timeout: Duration) -> Self {
        self.inline_request_timeout = timeout;
        self.mark("with_inline_request_timeout")
    }
}
