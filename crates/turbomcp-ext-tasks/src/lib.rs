//! TurboMCP v4 Tasks extension — `io.modelcontextprotocol/tasks` (SEP-2663).
//!
//! The draft (`2026-07-28`) moves Tasks out of the core protocol into an
//! **official extension**: a server may answer a `tools/call` with an
//! asynchronous *task handle* ([`wire::CreateTaskResult`],
//! `resultType: "task"`) instead of a final result, and the client polls
//! `tasks/get` / drives input via `tasks/update` / cancels via `tasks/cancel`.
//! This crate owns those wire types (the core draft schema defines none of
//! them) and plugs into the dispatcher through the [`Extension`] seam:
//!
//! ```ignore
//! use std::sync::Arc;
//! use turbomcp_ext_tasks::TasksExtension;
//!
//! let dispatcher = my_server
//!     .into_server()
//!     .with_tools()
//!     .with_extension(Arc::new(TasksExtension::new().task_tools(["slow_tool"])))
//!     .build();
//! ```
//!
//! Core Tasks for the legacy `2025-11-25` path (the different `tasks/list`/
//! `tasks/result` shape, session-scoped) is built into `turbomcp-server` and
//! is unaffected by this extension — the dispatcher serves whichever the
//! negotiated version calls for. Both keep tasks in a
//! [`TaskBackend`], and can share one: pass the
//! same backend to `ServerBuilder::with_task_backend` and
//! [`TasksExtension::backend`].
//!
//! ## Capability negotiation (SEP-2663)
//!
//! Task creation is **server-directed**: the client signals support by
//! declaring the extension in its per-request capabilities
//! (`_meta.io.modelcontextprotocol/clientCapabilities.extensions`), and the
//! server decides per request whether to materialize a task. A client that has
//! not declared the extension capability gets `-32021` for `tasks/*` (enforced
//! by the dispatcher before [`TasksExtension::dispatch`]) and is never returned
//! a `CreateTaskResult` (it always runs the call synchronously).
//!
//! ## Which calls become tasks
//!
//! By default a tool becomes a task when it says it can: `#[tool(task)]`
//! (`Tool::with_task_support`), the same marker that opts it into
//! `2025-11-25` core Tasks. `#[tool(task = "required")]` marks a tool that
//! can only run as a task; a client that hasn't declared the extension gets
//! `-32021` for it rather than a synchronous run. To decide otherwise, name
//! the tools with [`TasksExtension::task_tools`] or supply a predicate with
//! [`TasksExtension::task_policy`]; either replaces the default for every tool
//! but a required one, which has no synchronous path to fall back to.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use turbomcp_core::{
    JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestContext, RequestId,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallAugmentRequest, Extension, ExtensionRequest, NewTask, SubscribeOutcome, TaskBackend,
    TaskError, TaskLink, TaskOutcome, TaskOwner, TaskStore,
};

mod render;
mod subs;
pub mod wire;

use subs::TaskSubscriptions;
use wire::{CreateTaskResult, UpdateTaskParams};

/// The extension identifier, advertised under `server/discover`
/// `capabilities.extensions` and declared by clients to opt in.
pub const EXTENSION_ID: &str = "io.modelcontextprotocol/tasks";

/// Request methods this extension owns (SEP-2663 §Supported Methods).
pub mod methods {
    /// `tasks/get` — poll a task's current status (and, when terminal, its
    /// result or error inlined).
    pub const TASKS_GET: &str = "tasks/get";
    /// `tasks/update` — deliver `inputResponses` for an `input_required` task.
    pub const TASKS_UPDATE: &str = "tasks/update";
    /// `tasks/cancel` — request cancellation of an in-progress task.
    pub const TASKS_CANCEL: &str = "tasks/cancel";
}

const OWNED_METHODS: &[&str] = &[
    methods::TASKS_GET,
    methods::TASKS_UPDATE,
    methods::TASKS_CANCEL,
];

/// Default task retention, in milliseconds (5 minutes).
pub const DEFAULT_TTL_MS: i64 = 300_000;
/// Default suggested polling interval, in milliseconds.
pub const DEFAULT_POLL_INTERVAL_MS: i64 = 500;

/// Decides whether a given `tools/call` should run as a task. Receives the tool
/// name and the request context (identity, capabilities, …).
type TaskDecider = Arc<dyn Fn(&str, &RequestContext) -> bool + Send + Sync>;

/// The draft Tasks extension (`io.modelcontextprotocol/tasks`).
///
/// Register it with `ServerBuilder::with_extension(Arc::new(TasksExtension::new()))`.
#[derive(Clone)]
pub struct TasksExtension {
    store: Arc<dyn TaskBackend>,
    subs: Arc<TaskSubscriptions>,
    taskify: Option<TaskDecider>,
    ttl_ms: Option<i64>,
    poll_interval_ms: Option<i64>,
    capacity: usize,
    owner_limit: usize,
}

impl core::fmt::Debug for TasksExtension {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TasksExtension")
            // The decider is a user closure; whether one is installed is what
            // explains why a call did or did not become a task.
            .field("taskify", &self.taskify.is_some())
            .field("ttl_ms", &self.ttl_ms)
            .field("poll_interval_ms", &self.poll_interval_ms)
            .finish_non_exhaustive()
    }
}

impl Default for TasksExtension {
    fn default() -> Self {
        Self {
            store: Arc::new(TaskStore::default()),
            subs: Arc::new(TaskSubscriptions::default()),
            taskify: None,
            ttl_ms: Some(DEFAULT_TTL_MS),
            poll_interval_ms: Some(DEFAULT_POLL_INTERVAL_MS),
            capacity: TaskStore::DEFAULT_CAPACITY,
            owner_limit: TaskStore::DEFAULT_OWNER_LIMIT,
        }
    }
}

impl TasksExtension {
    /// Create the extension with an empty registry. Tools declared
    /// task-capable become tasks; [`task_tools`](Self::task_tools) /
    /// [`task_policy`](Self::task_policy) replace that rule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the named tools as tasks (when the client declared the extension),
    /// plus any tool that can only run as one.
    #[must_use]
    pub fn task_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let set: Vec<String> = names.into_iter().map(Into::into).collect();
        self.taskify = Some(Arc::new(move |name: &str, _ctx: &RequestContext| {
            set.iter().any(|n| n == name)
        }));
        self
    }

    /// Decide per call whether to taskify, with full access to the tool name and
    /// request context. Supersedes any prior [`task_tools`](Self::task_tools).
    /// A tool that can only run as a task is one regardless.
    #[must_use]
    pub fn task_policy<F>(mut self, policy: F) -> Self
    where
        F: Fn(&str, &RequestContext) -> bool + Send + Sync + 'static,
    {
        self.taskify = Some(Arc::new(policy));
        self
    }

    /// Override the task time-to-live in milliseconds (`None` ⇒ unlimited).
    /// Default: [`DEFAULT_TTL_MS`].
    ///
    /// A task still running when its TTL runs out is cancelled and marked
    /// `failed`; a finished one is kept for one more TTL, then deleted.
    ///
    /// # Panics
    ///
    /// On a TTL that is not positive: only `None` means unlimited, and zero or
    /// less would fail every task the moment it started.
    #[must_use]
    pub fn ttl_ms(mut self, ttl_ms: Option<i64>) -> Self {
        assert!(
            ttl_ms.is_none_or(|ms| ms > 0),
            "task TTL must be positive, or None for unlimited"
        );
        self.ttl_ms = ttl_ms;
        self
    }

    /// Override the suggested polling interval reported to clients, in
    /// milliseconds. Default: [`DEFAULT_POLL_INTERVAL_MS`].
    #[must_use]
    pub fn poll_interval_ms(mut self, poll_interval_ms: Option<i64>) -> Self {
        self.poll_interval_ms = poll_interval_ms;
        self
    }

    /// Bound the number of tasks the bundled registry holds (default: 1024).
    /// When it is full the oldest finished task makes room; when every task
    /// is still running, taskification degrades gracefully — the next
    /// eligible `tools/call` runs synchronously instead of failing. Replaces
    /// a [`backend`](Self::backend).
    #[must_use]
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self.store = Arc::new(self.bundled_store());
        self
    }

    /// Bound how many tasks one principal may have running at once in the
    /// bundled registry (default: 64), so a single caller cannot take every
    /// slot. Past it, that caller's eligible calls run synchronously.
    /// Replaces a [`backend`](Self::backend).
    #[must_use]
    pub fn owner_limit(mut self, limit: usize) -> Self {
        self.owner_limit = limit;
        self.store = Arc::new(self.bundled_store());
        self
    }

    /// Keep tasks in `backend` instead of the bundled in-memory registry: a
    /// shared store, so any replica can answer for a task, or the same
    /// backend `2025-11-25` core Tasks use
    /// (`ServerBuilder::with_task_backend`), so one registry fronts both
    /// wires. A task's work still runs on the replica that created it.
    #[must_use]
    pub fn backend(mut self, backend: Arc<dyn TaskBackend>) -> Self {
        self.store = backend;
        self
    }

    fn bundled_store(&self) -> TaskStore {
        TaskStore::default()
            .with_capacity(self.capacity)
            .with_owner_limit(self.owner_limit)
    }

    /// Whether `name` should run as a task under `ctx`.
    /// With no policy set, a tool declared task-capable (`#[tool(task)]`,
    /// `Tool::with_task_support`) becomes a task; a policy, when set, decides
    /// for the rest. A required tool is always one: declining it would only
    /// turn the call into an error.
    fn should_task(&self, tool: &neutral::Tool, ctx: &RequestContext) -> bool {
        use neutral::TaskSupport::{Optional, Required};
        match (&self.taskify, tool.task_support) {
            (_, Some(Required)) => true,
            (Some(decide), _) => decide(&tool.name, ctx),
            (None, support) => support == Some(Optional),
        }
    }
}

/// The `taskId`-only parameter shared by `tasks/get`/`tasks/cancel` (and the
/// `taskId` of `tasks/update`).
#[derive(Deserialize)]
struct TaskIdParams {
    #[serde(rename = "taskId")]
    task_id: String,
}

/// Parse the request's `taskId` (`-32602` on an absent/invalid one, SEP-2663
/// §Error Handling).
fn parse_task_id(request: &JsonRpcRequest) -> Result<String, JsonRpcError> {
    request
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<TaskIdParams>(p.clone()).ok())
        .map(|p| p.task_id)
        .ok_or_else(|| invalid_params("a `taskId` string is required"))
}

/// `-32602` (Invalid params).
fn invalid_params(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: turbomcp_core::codes::INVALID_PARAMS,
        message: message.into(),
        data: None,
    }
}

/// `tasks/*` for a `taskId` no live task matches (`-32602`, SEP-2663).
fn task_not_found(task_id: &str) -> JsonRpcError {
    invalid_params(format!("unknown task: {task_id}"))
}

/// `-32603` (Internal error).
fn internal(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: turbomcp_core::codes::INTERNAL_ERROR,
        message: message.into(),
        data: None,
    }
}

/// A backend failure on `tasks/*`: an unknown task is `-32602`, and anything
/// else is the server's problem.
fn task_error(task_id: &str, e: &TaskError) -> JsonRpcError {
    match e {
        TaskError::NotFound => task_not_found(task_id),
        other => {
            tracing::warn!(error = ?other, task = %task_id, "the task backend failed");
            internal("the task store failed")
        }
    }
}

/// Whose tasks a caller's are: theirs by principal, or the shared anonymous
/// bucket.
fn owner(context: &RequestContext) -> TaskOwner {
    TaskOwner::principal(context.identity.principal_key())
}

fn error(id: RequestId, err: JsonRpcError) -> JsonRpcMessage {
    JsonRpcResponse::error(id, err).into()
}

fn ok(id: RequestId, value: serde_json::Value) -> JsonRpcMessage {
    JsonRpcResponse::success(id, value).into()
}

/// The empty `resultType: "complete"` acknowledgement returned by
/// `tasks/update` and `tasks/cancel`.
fn ack(id: RequestId) -> JsonRpcMessage {
    ok(id, json!({ "resultType": wire::RESULT_TYPE_COMPLETE }))
}

#[async_trait]
impl Extension for TasksExtension {
    fn id(&self) -> &'static str {
        EXTENSION_ID
    }

    fn methods(&self) -> &'static [&'static str] {
        OWNED_METHODS
    }

    // With no policy, a tool's own `taskSupport` decides, so any call may be
    // one this extension takes.
    fn augments_calls(&self) -> bool {
        true
    }

    fn notification_topics(&self) -> &'static [&'static str] {
        &[subs::NOTIFICATIONS_TASKS]
    }

    async fn on_subscribe(
        &self,
        peer: &turbomcp_service::Peer,
        subscription_id: &turbomcp_core::RequestId,
        notifications: &serde_json::Value,
        client_declared: bool,
        context: &RequestContext,
    ) -> SubscribeOutcome {
        // The Tasks extension owns the `taskIds` filter on `subscriptions/listen`.
        let task_ids = notifications.get("taskIds").and_then(|v| v.as_array());
        let Some(task_ids) = task_ids.filter(|ids| !ids.is_empty()) else {
            return SubscribeOutcome::NotApplicable;
        };
        // SEP-2663: a client requesting task notifications without declaring the
        // extension capability is `-32021`.
        if !client_declared {
            return SubscribeOutcome::MissingCapability;
        }
        // Only the caller's own tasks: another's answers as if it didn't
        // exist.
        let owner = owner(context);
        let mut ids = Vec::new();
        for id in task_ids.iter().filter_map(serde_json::Value::as_str) {
            if self.store.get(&owner, id).await.is_ok() {
                ids.push(id.to_owned());
            }
        }
        let _ = (peer, subscription_id);
        SubscribeOutcome::Subscribed(json!({ "taskIds": ids }))
    }

    async fn activate(
        &self,
        peer: &turbomcp_service::Peer,
        subscription_id: &turbomcp_core::RequestId,
        accepted: &serde_json::Value,
        context: &RequestContext,
    ) {
        let ids: Vec<String> = accepted
            .get("taskIds")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        self.subs.forget_gone(self.store.as_ref()).await;
        self.subs
            .subscribe(peer, subscription_id, &ids, &owner(context));
    }

    fn on_unsubscribe(
        &self,
        connection: &turbomcp_core::ConnectionId,
        subscription_id: &turbomcp_core::RequestId,
    ) {
        self.subs.unsubscribe(connection, subscription_id);
    }

    async fn augment_call(&self, augment: CallAugmentRequest) -> Option<JsonRpcMessage> {
        // `CallAugmentRequest` is `#[non_exhaustive]`; take fields by access.
        let request = augment.request;
        let context = augment.context;
        let run = augment.run;
        if !self.should_task(&augment.tool, &context) {
            return None;
        }

        let cancel = run.cancel_token();
        // SEP-2663: a task MUST be durably created before `CreateTaskResult`
        // returns: create it, then spawn the call.
        let new_task = NewTask::new(self.ttl_ms).with_poll_interval_ms(self.poll_interval_ms);
        let task = match self
            .store
            .create(&owner(&context), new_task, cancel.clone())
            .await
        {
            Ok(task) => task,
            // Graceful degradation (SEP-2663): no task, so the call runs
            // synchronously.
            Err(e) => {
                tracing::warn!(error = ?e, "no task could be created; running the call inline");
                return None;
            }
        };

        // Bind the call to its task: `ctx.task`, `ctx.progress` and mid-task
        // client input (in-execution `input_required`, answered via
        // `tasks/update`) reach it, and each change it makes is pushed to
        // `subscriptions/listen` subscribers (spec-optional; pollers see it
        // via `tasks/get` regardless).
        let (store, subs) = (Arc::clone(&self.store), Arc::clone(&self.subs));
        run.attach_task(
            TaskLink::new(Arc::clone(&self.store), task.task_id.clone(), cancel).on_change(
                move |task_id| {
                    let (store, subs) = (Arc::clone(&store), Arc::clone(&subs));
                    async move { subs::push_status(&subs, store.as_ref(), &task_id).await }
                },
            ),
        );

        let store = Arc::clone(&self.store);
        let subs = Arc::clone(&self.subs);
        let task_id = task.task_id.clone();
        let span = tracing::info_span!("mcp.task", "mcp.task.id" = %task_id);
        let work = async move {
            // A panic would otherwise unwind this task with the record still
            // `working` — possibly forever, with an unlimited TTL. Every other
            // path answers a panicking handler `-32603`; so does this.
            // A tool-level `isError: true` is still a `completed` task here,
            // unlike on `2025-11-25`.
            let outcome = match turbomcp_service::catch_panic(run.run()).await {
                Ok(Ok(result)) => TaskOutcome::Completed(result),
                Ok(Err(err)) => TaskOutcome::Error(err),
                Err(panic) => {
                    tracing::error!(panic, task = %task_id, "task handler panicked");
                    TaskOutcome::Error(turbomcp_core::JsonRpcError {
                        code: turbomcp_core::codes::INTERNAL_ERROR,
                        message: "handler panicked".to_owned(),
                        data: None,
                    })
                }
            };
            store.complete(&task_id, outcome).await;
            // Push the terminal status to any `subscriptions/listen` subscribers
            // (spec-optional; pollers see it via `tasks/get` regardless).
            subs::push_status(&subs, store.as_ref(), &task_id).await;
        };
        // The work runs in a span of its own, parented to the call that
        // created it: the call's span ends as soon as the task is created,
        // and the work used to run outside any span at all.
        tokio::spawn(tracing::Instrument::instrument(work, span));

        let value = serde_json::to_value(CreateTaskResult::new(render::task(&task))).ok()?;
        Some(ok(request.id, value))
    }

    async fn dispatch(&self, request: ExtensionRequest) -> JsonRpcMessage {
        let ExtensionRequest {
            request, context, ..
        } = request;
        let id = request.id.clone();

        let task_id = match parse_task_id(&request) {
            Ok(t) => t,
            Err(e) => return error(id, e),
        };

        let owner = owner(&context);
        match request.method.as_str() {
            methods::TASKS_GET => match self.store.get(&owner, &task_id).await {
                Ok(snapshot) => match serde_json::to_value(render::detailed(snapshot)) {
                    Ok(value) => ok(id, value),
                    Err(e) => error(id, internal(format!("serialize task: {e}"))),
                },
                Err(e) => error(id, task_error(&task_id, &e)),
            },
            // `tasks/cancel` acks unconditionally for a known task (cooperative,
            // eventually consistent); unknown ⇒ `-32602` (SHOULD).
            methods::TASKS_CANCEL => match self.store.cancel(&owner, &task_id).await {
                Ok(_) => {
                    subs::push_status(&self.subs, self.store.as_ref(), &task_id).await;
                    ack(id)
                }
                Err(TaskError::AlreadyTerminal) => ack(id),
                Err(e) => error(id, task_error(&task_id, &e)),
            },
            // `tasks/update` delivers `inputResponses` to the awaiting
            // handler. Responses for keys that aren't currently outstanding —
            // never issued, already answered, superseded — are ignored, and a
            // partial set is accepted (spec). The empty ack MAY precede the
            // observable status change (eventual consistency); here delivery
            // is synchronous, which is strictly stronger.
            methods::TASKS_UPDATE => {
                let responses = request
                    .params
                    .as_ref()
                    .and_then(|p| serde_json::from_value::<UpdateTaskParams>(p.clone()).ok())
                    .map(|p| p.input_responses)
                    .unwrap_or_default();
                match self.store.provide_input(&owner, &task_id, &responses).await {
                    Ok(changed) => {
                        if changed {
                            // Announce the flip back to `working`.
                            subs::push_status(&self.subs, self.store.as_ref(), &task_id).await;
                        }
                        ack(id)
                    }
                    Err(e) => error(id, task_error(&task_id, &e)),
                }
            }
            // The dispatcher only routes our declared methods here.
            other => error(
                id,
                JsonRpcError {
                    code: turbomcp_core::codes::METHOD_NOT_FOUND,
                    message: format!("method not found: {other}"),
                    data: None,
                },
            ),
        }
    }
}
