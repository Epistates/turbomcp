//! The [`Extension`] seam: a multi-method server plugin (PLAN D10).
//!
//! An extension owns a set of request methods (e.g. the Tasks extension's
//! `tasks/get`/`tasks/update`/`tasks/cancel`), advertises itself under
//! `capabilities.extensions[id]` (SEP-2133: in `initialize` on the stateful
//! revisions, in `server/discover` on the stateless one), and is dispatched by
//! the [`VersionDispatcher`](crate::VersionDispatcher) on the revisions it
//! speaks ([`Extension::protocol_versions`], `2026-07-28` by default) once the
//! client has declared it: per request on `2026-07-28`, in `initialize` on a
//! session. Handlers ask with
//! [`RequestContext::supports_extension`](turbomcp_core::RequestContext::supports_extension).
//!
//! A revision whose core protocol defines a method keeps it: the Tasks
//! extension speaks only `2026-07-28`, and `2025-11-25` serves its built-in
//! core Tasks instead. Registration refuses collisions. The trait is object-safe and
//! dispatched behind `Arc<dyn Extension>`, so extensions live in their own
//! crates (e.g. `turbomcp-ext-tasks`) and register via
//! [`ServerBuilder::with_extension`](crate::ServerBuilder::with_extension) /
//! [`VersionDispatcher::with_extension`](crate::VersionDispatcher::with_extension).
//!
//! The trait is the durable architectural asset; the D10 sketch's
//! `intercept_response`/`notification_topics` are folded into the real seams an
//! extension actually needs — [`dispatch`](Extension::dispatch) for its owned
//! methods, and (Phase 9b) a task-augmentation hook for `tools/call` — rather
//! than modeled as standalone trait methods with no consumer.

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::Value;
use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcRequest, RequestContext,
};

use turbomcp_core::ProtocolVersion;
use turbomcp_protocol::methods::request;

use crate::task_handle::{TaskLink, TaskSlot};

/// One inbound request routed to an [`Extension`]. The dispatcher has already
/// version-gated it to the modern path and verified the client declared the
/// extension capability, so a handler can trust both.
#[non_exhaustive]
#[derive(Debug)]
pub struct ExtensionRequest {
    /// The raw JSON-RPC request; its method is one of [`Extension::methods`].
    pub request: JsonRpcRequest,
    /// The per-request context (version, identity, client capabilities, …).
    /// Its `extensions` carry the transport's facts, including the
    /// [`Peer`](turbomcp_service::Peer) an extension pushes notifications to.
    pub context: RequestContext,
}

/// The underlying `tools/call`, prepared for an extension to run as a task.
///
/// The dispatcher builds the call's handler future (with the task's
/// cancellation token already wired into its context) and hands it over. An
/// extension that decides to taskify the call reads [`cancel_token`] (to drive
/// `tasks/cancel`), registers the task, attaches a [`TaskLink`] to it (so the
/// handler's `ctx.task`, `ctx.progress` and mid-task client input reach the
/// task), and spawns [`run`] in the background; the future resolves to the
/// wire `CallToolResult` JSON on success, or the JSON-RPC error the call
/// would have answered with.
///
/// [`cancel_token`]: CallRunner::cancel_token
/// [`run`]: CallRunner::run
pub struct CallRunner {
    future: BoxFuture<'static, Result<Value, JsonRpcError>>,
    cancel: CancellationToken,
    task: TaskSlot,
}

impl core::fmt::Debug for CallRunner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CallRunner")
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl CallRunner {
    /// Wrap a prepared call future and the cancellation token wired into it.
    /// (Constructed by the dispatcher; extensions consume one via
    /// [`CallAugmentRequest`].)
    #[must_use]
    pub fn new(
        future: BoxFuture<'static, Result<Value, JsonRpcError>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            future,
            cancel,
            task: TaskSlot::default(),
        }
    }

    /// Share the slot already wired into the call's context.
    #[must_use]
    pub(crate) fn with_task_slot(mut self, slot: TaskSlot) -> Self {
        self.task = slot;
        self
    }

    /// Bind the call to the task it now runs as. Call **before** spawning
    /// [`run`](Self::run); a second attach is a no-op (first wins).
    pub fn attach_task(&self, link: TaskLink) {
        let _ = self.task.set(link);
    }

    /// The cancellation token wired into the call — fire it from `tasks/cancel`
    /// (or a TTL purge) to ask the handler to stop.
    #[must_use]
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Drive the underlying call to completion. The result is the wire
    /// `CallToolResult` JSON (a tool-level `isError: true` is still `Ok` — that
    /// is a `completed` task, not a `failed` one) or the JSON-RPC error.
    pub async fn run(self) -> Result<Value, JsonRpcError> {
        self.future.await
    }
}

/// A `tools/call` offered to a call-augmenting [`Extension`]. The dispatcher
/// only constructs this for clients that declared the extension capability, so
/// returning a `CreateTaskResult` honors SEP-2663's "MUST NOT task a
/// non-declaring client".
#[non_exhaustive]
#[derive(Debug)]
pub struct CallAugmentRequest {
    /// The `tools/call` request.
    pub request: JsonRpcRequest,
    /// The per-request context. Its `extensions` carry the
    /// [`Peer`](turbomcp_service::Peer) for pushing `notifications/tasks`.
    pub context: RequestContext,
    /// The prepared underlying call (spawn it if you take over the request).
    pub run: CallRunner,
    /// The tool being called, as the server lists it: its `task_support`
    /// says whether it may (or must) run as a task.
    pub tool: turbomcp_protocol::neutral::Tool,
}

/// The result of offering a `subscriptions/listen` request to an extension
/// (SEP-2663 task-status notifications ride this stream).
#[derive(Debug)]
pub enum SubscribeOutcome {
    /// The listen request doesn't reference this extension's notifications.
    NotApplicable,
    /// The request targets the extension but the client didn't declare its
    /// capability → the dispatcher answers `-32021` (Missing Required Client
    /// Capability), per SEP-2663.
    MissingCapability,
    /// The extension agreed; the returned object is merged into the
    /// acknowledgement's `notifications` (echoing the filters the server
    /// agreed to honor) and handed back to
    /// [`Extension::activate`] once the acknowledgement is queued.
    Subscribed(Value),
}

/// A multi-method server extension (PLAN D10).
///
/// An extension bundles a cohesive feature that lives outside the core protocol
/// surface — owning its own wire types and request methods — and plugs into the
/// dispatcher without the core needing to know about it. The draft Tasks
/// extension (`io.modelcontextprotocol/tasks`, SEP-2663) is the reference
/// implementation; see `turbomcp-ext-tasks`.
#[async_trait]
pub trait Extension: Send + Sync + 'static {
    /// The stable extension identifier, e.g. `io.modelcontextprotocol/tasks`.
    /// Used as the key under `server/discover` `capabilities.extensions` and as
    /// the per-request capability a client must declare to use the extension.
    fn id(&self) -> &'static str;

    /// The settings object advertised under `capabilities.extensions[id]`.
    /// Defaults to an empty object — "supported, with no settings".
    fn settings(&self) -> Value {
        Value::Object(serde_json::Map::new())
    }

    /// The request methods this extension owns. On the revisions it speaks
    /// ([`protocol_versions`](Extension::protocol_versions)) the dispatcher
    /// routes these to [`dispatch`](Extension::dispatch); a client that has
    /// not declared the extension gets Missing Required Client Capability
    /// (`-32021`; `-32602` on the stateful revisions) naming it, so it can
    /// declare and retry. None may be a method the core protocol defines on
    /// those revisions, nor one another extension claims there.
    fn methods(&self) -> &'static [&'static str];

    /// The revisions this extension speaks. It is advertised only to a
    /// client on one of them (in `initialize` on the stateful revisions,
    /// SEP-2133, and in `server/discover` on the stateless one), and its
    /// methods are routed only there. Defaults to `2026-07-28`, the revision
    /// this seam began on.
    ///
    /// An extension that only shapes what the core methods carry (Apps: UI
    /// metadata on tools and resources) speaks every revision; one whose
    /// methods a revision defines itself must not claim that revision (the
    /// Tasks extension's `tasks/*` are core on `2025-11-25`).
    fn protocol_versions(&self) -> &'static [ProtocolVersion] {
        &[ProtocolVersion::V2026_07_28]
    }

    /// Handle one of the extension's [`methods`](Extension::methods) and return
    /// the JSON-RPC response. The dispatcher guarantees the request's method is
    /// one this extension declared and that the client declared the extension
    /// capability.
    async fn dispatch(&self, request: ExtensionRequest) -> JsonRpcMessage;

    /// Whether this extension may convert a `tools/call` into a task. A cheap
    /// pre-check: the dispatcher only prepares a [`CallRunner`] (and consults
    /// [`augment_call`](Extension::augment_call)) when this returns `true`.
    /// Defaults to `false`.
    fn augments_calls(&self) -> bool {
        false
    }

    /// Offer a `tools/call` for task augmentation (SEP-2663). Return
    /// `Some(response)` to take over the request — a `CreateTaskResult` after
    /// spawning [`CallAugmentRequest::run`] in the background — or `None` to let
    /// the dispatcher run the call normally. Only invoked for clients that
    /// declared the extension capability. Defaults to `None` (never taskify).
    async fn augment_call(&self, _request: CallAugmentRequest) -> Option<JsonRpcMessage> {
        None
    }

    /// Notification methods this extension may push on `subscriptions/listen`
    /// streams (e.g. `notifications/tasks`). Informational — surfaced for
    /// introspection; the extension itself pushes through the subscription's
    /// [`Peer`](turbomcp_service::Peer). Defaults to none.
    fn notification_topics(&self) -> &'static [&'static str] {
        &[]
    }

    /// Offer a `subscriptions/listen` request to the extension. `notifications`
    /// is the raw filter object from the request (so the extension reads its own
    /// fields, e.g. the Tasks extension's `taskIds`); `subscription_id` is the
    /// listen request's JSON-RPC id — every notification the extension later
    /// pushes on this subscription MUST carry it verbatim in
    /// `_meta["io.modelcontextprotocol/subscriptionId"]`; `client_declared` is
    /// whether the client declared this extension's capability. Returns a
    /// [`SubscribeOutcome`]; defaults to [`SubscribeOutcome::NotApplicable`].
    ///
    /// Decide here; don't start sending. "The server MUST NOT send any
    /// notification on the subscription before" its acknowledgement, which
    /// goes out after every extension has answered (another may yet refuse
    /// the listen). [`activate`](Self::activate) is when to start.
    async fn on_subscribe(
        &self,
        _peer: &turbomcp_service::Peer,
        _subscription_id: &turbomcp_core::RequestId,
        _notifications: &Value,
        _client_declared: bool,
        _context: &RequestContext,
    ) -> SubscribeOutcome {
        SubscribeOutcome::NotApplicable
    }

    /// Start sending what [`on_subscribe`](Self::on_subscribe) agreed to:
    /// `accepted` is the object it returned in
    /// [`SubscribeOutcome::Subscribed`], and the acknowledgement is already
    /// queued on `peer` ahead of anything sent from now on. `context` is the
    /// listen request's, as `on_subscribe` saw it. Defaults to nothing.
    async fn activate(
        &self,
        _peer: &turbomcp_service::Peer,
        _subscription_id: &turbomcp_core::RequestId,
        _accepted: &Value,
        _context: &RequestContext,
    ) {
    }

    /// The client ended subscription `subscription_id` on `connection`
    /// (`notifications/cancelled` naming its listen request): stop sending on
    /// it and forget it. A connection that closes is not reported here; its
    /// peer reads closed. Defaults to nothing.
    fn on_unsubscribe(
        &self,
        _connection: &turbomcp_core::ConnectionId,
        _subscription_id: &turbomcp_core::RequestId,
    ) {
    }
}

/// The request methods the core protocol defines on `version`, which no
/// extension may claim there.
fn core_methods(version: &ProtocolVersion) -> &'static [&'static str] {
    use request::*;
    const STATELESS: &[&str] = &[
        DISCOVER,
        SUBSCRIPTIONS_LISTEN,
        TOOLS_LIST,
        TOOLS_CALL,
        RESOURCES_LIST,
        RESOURCES_TEMPLATES_LIST,
        RESOURCES_READ,
        PROMPTS_LIST,
        PROMPTS_GET,
        COMPLETION_COMPLETE,
    ];
    const STATEFUL: &[&str] = &[
        INITIALIZE,
        PING,
        RESOURCES_SUBSCRIBE,
        RESOURCES_UNSUBSCRIBE,
        LOGGING_SET_LEVEL,
        TOOLS_LIST,
        TOOLS_CALL,
        RESOURCES_LIST,
        RESOURCES_TEMPLATES_LIST,
        RESOURCES_READ,
        PROMPTS_LIST,
        PROMPTS_GET,
        COMPLETION_COMPLETE,
    ];
    // `2025-11-25` is the stateful set plus core Tasks.
    const WITH_TASKS: &[&str] = &[
        INITIALIZE,
        PING,
        RESOURCES_SUBSCRIBE,
        RESOURCES_UNSUBSCRIBE,
        LOGGING_SET_LEVEL,
        TOOLS_LIST,
        TOOLS_CALL,
        RESOURCES_LIST,
        RESOURCES_TEMPLATES_LIST,
        RESOURCES_READ,
        PROMPTS_LIST,
        PROMPTS_GET,
        COMPLETION_COMPLETE,
        TASKS_LIST,
        TASKS_GET,
        TASKS_CANCEL,
        TASKS_RESULT,
    ];
    match version {
        ProtocolVersion::V2025_11_25 => WITH_TASKS,
        v if v.is_stateless() => STATELESS,
        _ => STATEFUL,
    }
}

/// Requests the server sends the client: never a method an extension serves.
const CLIENT_METHODS: [&str; 3] = [
    request::ELICITATION_CREATE,
    request::SAMPLING_CREATE_MESSAGE,
    request::ROOTS_LIST,
];

/// Refuse to register `new` beside `existing` when they would collide.
/// "Last-write-wins is how plugins corrupt each other": a silent
/// first-match-wins let an extension claiming `tools/call` take over every
/// call.
///
/// # Panics
/// On a second extension with the same id, a method the core protocol
/// defines on a revision `new` speaks, or a method another extension
/// already claims on a revision both speak.
pub(crate) fn check_registration(existing: &[Arc<dyn Extension>], new: &dyn Extension) {
    let id = new.id();
    assert!(
        !existing.iter().any(|e| e.id() == id),
        "extension `{id}` is registered twice"
    );
    for version in new.protocol_versions() {
        for method in new.methods() {
            assert!(
                !core_methods(version).contains(method) && !CLIENT_METHODS.contains(method),
                "extension `{id}` claims `{method}`, which the core protocol defines on {version}"
            );
            if let Some(other) = existing
                .iter()
                .find(|e| e.protocol_versions().contains(version) && e.methods().contains(method))
            {
                panic!(
                    "extensions `{}` and `{id}` both claim `{method}` on {version}",
                    other.id()
                );
            }
        }
    }
}
