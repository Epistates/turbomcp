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
//! negotiated version calls for.
//!
//! ## Capability negotiation (SEP-2663)
//!
//! Task creation is **server-directed**: the client signals support by
//! declaring the extension in its per-request capabilities
//! (`_meta.io.modelcontextprotocol/clientCapabilities.extensions`), and the
//! server decides per request whether to materialize a task. A client that has
//! not declared the extension capability gets `-32601` for `tasks/*` (enforced
//! by the dispatcher before [`TasksExtension::dispatch`]) and is never returned
//! a `CreateTaskResult` (it always runs the call synchronously).
//!
//! ## Which calls become tasks
//!
//! By default **no** `tools/call` is taskified (existing behavior is
//! unchanged). Opt specific tools in with [`TasksExtension::task_tools`], or
//! supply an arbitrary predicate with [`TasksExtension::task_policy`]. The
//! server is the sole decider; a declared client never *requires* a task.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpError,
    McpResult, RequestContext, RequestId,
};
use turbomcp_server::{
    CallAugmentRequest, Extension, ExtensionRequest, SubscribeOutcome, TaskInputBroker,
};

mod store;
mod subs;
pub mod wire;

use store::{DraftTaskStore, TaskOutcome};
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
    store: Arc<DraftTaskStore>,
    subs: Arc<TaskSubscriptions>,
    taskify: Option<TaskDecider>,
    ttl_ms: Option<i64>,
    poll_interval_ms: Option<i64>,
}

impl Default for TasksExtension {
    fn default() -> Self {
        Self {
            store: Arc::new(DraftTaskStore::default()),
            subs: Arc::new(TaskSubscriptions::default()),
            taskify: None,
            ttl_ms: Some(DEFAULT_TTL_MS),
            poll_interval_ms: Some(DEFAULT_POLL_INTERVAL_MS),
        }
    }
}

impl TasksExtension {
    /// Create the extension with an empty registry. No `tools/call` is taskified
    /// until you opt tools in via [`task_tools`](Self::task_tools) /
    /// [`task_policy`](Self::task_policy).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the named tools as tasks (when the client declared the extension).
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
    #[must_use]
    pub fn ttl_ms(mut self, ttl_ms: Option<i64>) -> Self {
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

    /// Bound the number of live tasks the registry holds (default: 1024).
    /// At capacity, taskification degrades gracefully — the next eligible
    /// `tools/call` runs synchronously instead of failing — until an expiry
    /// or terminal purge frees a slot. Call before registration.
    #[must_use]
    pub fn capacity(mut self, capacity: usize) -> Self {
        self.store = Arc::new(DraftTaskStore::with_capacity(capacity));
        self
    }

    /// Whether `name` should run as a task under `ctx`.
    fn should_task(&self, name: &str, ctx: &RequestContext) -> bool {
        self.taskify
            .as_ref()
            .is_some_and(|decide| decide(name, ctx))
    }
}

/// The `taskId`-only parameter shared by `tasks/get`/`tasks/cancel` (and the
/// `taskId` of `tasks/update`).
#[derive(Deserialize)]
struct TaskIdParams {
    #[serde(rename = "taskId")]
    task_id: String,
}

/// The `name` field of a `tools/call`, to decide taskification.
#[derive(Deserialize)]
struct CallToolName {
    name: String,
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
        code: -32602,
        message: message.into(),
        data: None,
    }
}

/// `tasks/*` for a `taskId` no live task matches (`-32602`, SEP-2663).
fn task_not_found(task_id: &str) -> JsonRpcError {
    invalid_params(format!("unknown task: {task_id}"))
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

/// One task's [`TaskInputBroker`] (SEP-2663 in-execution `input_required`):
/// publishes the handler's input requests into the task's store entry —
/// flipping it to `input_required`, surfaced by `tasks/get` — pushes the
/// status to subscribers, and awaits the client's `tasks/update` answer.
/// Cancellation (`tasks/cancel`, TTL purge) unblocks the awaiting handler
/// with an error so it unwinds.
struct TaskBroker {
    store: Arc<DraftTaskStore>,
    subs: Arc<TaskSubscriptions>,
    task_id: String,
    cancel: CancellationToken,
}

impl TaskInputBroker for TaskBroker {
    fn obtain(
        &self,
        key: &str,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<'static, McpResult<serde_json::Value>> {
        let store = Arc::clone(&self.store);
        let subs = Arc::clone(&self.subs);
        let task_id = self.task_id.clone();
        let cancel = self.cancel.clone();
        let key = key.to_owned();
        Box::pin(async move {
            let rx = store.publish_input(&task_id, &key, request).map_err(|()| {
                McpError::internal("the task is no longer live; client input is unavailable")
            })?;
            // Announce `input_required` to any listen-stream subscribers
            // (spec-optional; pollers see it via `tasks/get` regardless).
            subs::push_status(&subs, &store, &task_id).await;
            tokio::select! {
                () = cancel.cancelled() => Err(McpError::internal(
                    "the task was cancelled while awaiting client input",
                )),
                response = rx => response.map_err(|_| {
                    McpError::internal("the task input channel closed before a response arrived")
                }),
            }
        })
    }
}

#[async_trait]
impl Extension for TasksExtension {
    fn id(&self) -> &'static str {
        EXTENSION_ID
    }

    fn methods(&self) -> &'static [&'static str] {
        OWNED_METHODS
    }

    fn augments_calls(&self) -> bool {
        self.taskify.is_some()
    }

    fn notification_topics(&self) -> &'static [&'static str] {
        &[subs::NOTIFICATIONS_TASKS]
    }

    fn on_subscribe(
        &self,
        connection_id: &str,
        subscription_id: &turbomcp_core::RequestId,
        notifications: &serde_json::Value,
        client_declared: bool,
    ) -> SubscribeOutcome {
        // The Tasks extension owns the `taskIds` filter on `subscriptions/listen`.
        let task_ids = notifications.get("taskIds").and_then(|v| v.as_array());
        let Some(task_ids) = task_ids.filter(|ids| !ids.is_empty()) else {
            return SubscribeOutcome::NotApplicable;
        };
        // SEP-2663: a client requesting task notifications without declaring the
        // extension capability is `-32003`.
        if !client_declared {
            return SubscribeOutcome::MissingCapability;
        }
        let ids: Vec<String> = task_ids
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        self.subs.subscribe(connection_id, subscription_id, &ids);
        SubscribeOutcome::Subscribed(json!({ "taskIds": ids }))
    }

    async fn augment_call(&self, augment: CallAugmentRequest) -> Option<JsonRpcMessage> {
        // `CallAugmentRequest` is `#[non_exhaustive]`; take fields by access.
        let request = augment.request;
        let context = augment.context;
        let run = augment.run;

        // Decide from the tool name; an unparseable call falls through to the
        // normal dispatch (which answers `-32602` for the bad envelope).
        let name = request
            .params
            .as_ref()
            .and_then(|p| serde_json::from_value::<CallToolName>(p.clone()).ok())?
            .name;
        if !self.should_task(&name, &context) {
            return None;
        }

        let cancel = run.cancel_token();
        // SEP-2663: a task MUST be durably created before `CreateTaskResult`
        // returns. We create synchronously here, then spawn the call.
        let task = self
            .store
            .create(self.ttl_ms, self.poll_interval_ms, cancel.clone())?; // capacity ⇒ run normally

        // Enable mid-task client input (in-execution `input_required`): the
        // call's ClientHandle publishes through this broker; the client
        // answers via `tasks/update`.
        run.attach_input_broker(Arc::new(TaskBroker {
            store: Arc::clone(&self.store),
            subs: Arc::clone(&self.subs),
            task_id: task.task_id.clone(),
            cancel,
        }));

        let store = Arc::clone(&self.store);
        let subs = Arc::clone(&self.subs);
        let task_id = task.task_id.clone();
        tokio::spawn(async move {
            let outcome = match run.run().await {
                Ok(result) => TaskOutcome::Completed(result),
                Err(err) => TaskOutcome::Failed(err),
            };
            store.complete(&task_id, outcome);
            // Push the terminal status to any `subscriptions/listen` subscribers
            // (spec-optional; pollers see it via `tasks/get` regardless).
            subs::push_status(&subs, &store, &task_id).await;
        });

        let value = serde_json::to_value(CreateTaskResult::new(task)).ok()?;
        Some(ok(request.id, value))
    }

    async fn dispatch(&self, request: ExtensionRequest) -> JsonRpcMessage {
        let ExtensionRequest { request, .. } = request;
        let id = request.id.clone();

        let task_id = match parse_task_id(&request) {
            Ok(t) => t,
            Err(e) => return error(id, e),
        };

        match request.method.as_str() {
            methods::TASKS_GET => match self.store.get(&task_id) {
                Some(detailed) => match serde_json::to_value(detailed) {
                    Ok(value) => ok(id, value),
                    Err(e) => error(
                        id,
                        JsonRpcError {
                            code: -32603,
                            message: format!("serialize task: {e}"),
                            data: None,
                        },
                    ),
                },
                None => error(id, task_not_found(&task_id)),
            },
            // `tasks/cancel` acks unconditionally for a known task (cooperative,
            // eventually consistent); unknown ⇒ `-32602` (SHOULD).
            methods::TASKS_CANCEL => {
                if self.store.cancel(&task_id) {
                    subs::push_status(&self.subs, &self.store, &task_id).await;
                    ack(id)
                } else {
                    error(id, task_not_found(&task_id))
                }
            }
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
                match self.store.deliver_inputs(&task_id, &responses) {
                    Some(changed) => {
                        if changed {
                            // Announce the flip back to `working`.
                            subs::push_status(&self.subs, &self.store, &task_id).await;
                        }
                        ack(id)
                    }
                    None => error(id, task_not_found(&task_id)),
                }
            }
            // The dispatcher only routes our declared methods here.
            other => error(
                id,
                JsonRpcError {
                    code: -32601,
                    message: format!("method not found: {other}"),
                    data: None,
                },
            ),
        }
    }
}
