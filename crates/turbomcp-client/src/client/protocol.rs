//! Protocol client for JSON-RPC communication
//!
//! This module provides the ProtocolClient which handles the low-level
//! JSON-RPC protocol communication with MCP servers.
//!
//! ## Bidirectional Communication Architecture
//!
//! The ProtocolClient uses a MessageDispatcher to solve the bidirectional
//! communication problem. Instead of directly calling `transport.receive()`,
//! which created race conditions when multiple code paths tried to receive,
//! we now use a centralized message routing layer:
//!
//! ```text
//! ProtocolClient::request()
//!     ↓
//!   1. Register oneshot channel with dispatcher
//!   2. Send request via transport
//!   3. Wait on oneshot channel
//!     ↓
//! MessageDispatcher (background task)
//!     ↓
//!   Continuously reads transport.receive()
//!   Routes responses → oneshot channels
//!   Routes requests → Client handlers
//! ```
//!
//! This ensures there's only ONE consumer of transport.receive(),
//! eliminating the race condition.

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Notify, oneshot};
use turbomcp_protocol::jsonrpc::{JsonRpcResponse, JsonRpcVersion};
use turbomcp_protocol::types::ProgressToken;
use turbomcp_protocol::{Error, MessageId, Result};
use turbomcp_transport::config::TimeoutConfig;
use turbomcp_transport::{Transport, TransportConfig, TransportError, TransportMessage};

use super::dispatcher::{MessageDispatcher, WaiterGuard};

/// How long a request may wait for its response.
#[derive(Debug, Clone, Copy, Default)]
pub(super) enum Deadline {
    /// The transport configuration's `timeouts.request`, capped by
    /// `timeouts.total`.
    #[default]
    Configured,
    /// This long instead of `timeouts.request`. The `timeouts.total` cap still
    /// applies, but never below this.
    Within(Duration),
    /// No limit at all, for requests the specification lets block until
    /// something happens on the server — `tasks/result` waits for the task.
    #[cfg_attr(not(feature = "experimental-tasks"), allow(dead_code))]
    Unbounded,
}

impl Deadline {
    /// The per-request timeout and the overall cap, in that order.
    fn resolve(self, timeouts: &TimeoutConfig) -> (Option<Duration>, Option<Duration>) {
        match self {
            Self::Configured => (timeouts.request, timeouts.total),
            Self::Within(timeout) => (Some(timeout), timeouts.total.map(|t| t.max(timeout))),
            Self::Unbounded => (None, None),
        }
    }
}

/// Per-request options for [`ProtocolClient::request_with`].
#[derive(Debug, Default)]
pub(super) struct RequestOptions {
    /// How long to wait for the response.
    pub(super) deadline: Deadline,
    /// The `_meta.progressToken` the request carries, if any.
    ///
    /// The token is tracked while the request is in flight: notifications
    /// for it restart the request timeout, and notifications for any token
    /// not tracked are ignored.
    pub(super) progress_token: Option<ProgressToken>,
    /// Whether the request carries `task` augmentation.
    ///
    /// cancellation.mdx: a task-augmented request is cancelled with
    /// `tasks/cancel`, never `notifications/cancelled`; and its progress
    /// token stays live, past the response, for the task it created.
    pub(super) task_augmented: bool,
}

impl RequestOptions {
    /// Options that set only the deadline.
    pub(super) fn with_deadline(deadline: Deadline) -> Self {
        Self {
            deadline,
            ..Self::default()
        }
    }
}

/// Why a request failed.
#[derive(Debug)]
pub(super) enum RequestError {
    /// The transport reported that the server no longer knows the session
    /// ([`TransportError::SessionExpired`]). Kept apart from every other
    /// failure so the client can start a new session and retry — which it
    /// could not do once the error had been flattened into a string.
    SessionExpired(Error),
    /// Any other failure.
    Failed(Error),
}

impl From<Error> for RequestError {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

impl From<serde_json::Error> for RequestError {
    fn from(error: serde_json::Error) -> Self {
        Self::Failed(error.into())
    }
}

impl From<RequestError> for Error {
    fn from(error: RequestError) -> Self {
        match error {
            RequestError::SessionExpired(error) | RequestError::Failed(error) => error,
        }
    }
}

/// A JSON-RPC request serialized from borrowed parts, so the params a caller
/// may need again — to retry on a new session — are not cloned to send them.
#[derive(Serialize)]
struct OutgoingRequest<'a> {
    jsonrpc: JsonRpcVersion,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a serde_json::Value>,
    id: &'a MessageId,
}

/// JSON-RPC protocol handler for MCP communication
///
/// Handles request/response correlation, serialization, and protocol-level concerns.
/// This is the abstraction layer between raw Transport and high-level Client APIs.
///
/// ## Architecture
///
/// The ProtocolClient now uses a MessageDispatcher to handle bidirectional
/// communication correctly. The dispatcher runs a background task that:
/// - Reads ALL messages from the transport
/// - Routes responses to waiting request() calls
/// - Routes incoming requests to registered handlers
///
/// This eliminates race conditions by centralizing all message routing
/// in a single background task.
#[derive(Debug)]
pub(super) struct ProtocolClient<T: Transport> {
    transport: Arc<T>,
    dispatcher: Arc<MessageDispatcher>,
    /// Shared with the tasks that cancel abandoned task-augmented requests,
    /// which send a `tasks/cancel` of their own after the caller has gone.
    next_id: Arc<AtomicU64>,
    /// Transport configuration for timeout enforcement (v2.2.0+)
    config: TransportConfig,
    /// Elicitation ids this client has been told about and not yet seen
    /// completed — from URL-mode `elicitation/create` requests, and from the
    /// `data.elicitations` of a -32042 error answering one of our requests.
    ///
    /// The spec requires a client to ignore
    /// `notifications/elicitation/complete` for an unknown or
    /// already-completed id — otherwise a server (or anything able to inject a
    /// notification) can drive a client's retry logic with an id it invented.
    /// It lives here rather than on the client because this is the layer
    /// that sees error responses.
    url_elicitations: Mutex<HashSet<String>>,
    /// Progress tokens of requests in flight, and of the tasks they created.
    progress: ProgressTokens,
}

impl<T: Transport + 'static> ProtocolClient<T> {
    /// Create a new protocol client with custom transport configuration
    ///
    /// This allows setting custom timeouts and limits.
    pub(super) fn with_config(transport: T, config: TransportConfig) -> Self {
        let transport = Arc::new(transport);
        let dispatcher = MessageDispatcher::new(transport.clone());

        Self {
            transport,
            dispatcher,
            next_id: Arc::new(AtomicU64::new(1)),
            config,
            url_elicitations: Mutex::new(HashSet::new()),
            progress: ProgressTokens::default(),
        }
    }

    /// Remember a URL-mode elicitation id so its completion is honoured.
    pub(super) fn track_url_elicitation(&self, elicitation_id: String) {
        self.url_elicitations.lock().insert(elicitation_id);
    }

    /// Track every elicitation id in a -32042 error's `data`.
    ///
    /// A client retries the failed request once these complete, so their
    /// completions have to be honoured just like those of an
    /// `elicitation/create` it received directly.
    fn track_elicitations_in(&self, data: &serde_json::Value) {
        let ids = data
            .get("elicitations")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("elicitationId").and_then(serde_json::Value::as_str));
        let mut tracked = self.url_elicitations.lock();
        for id in ids {
            tracked.insert(id.to_owned());
        }
    }

    /// Consume a URL-mode elicitation id. `false` means the id is unknown or
    /// already completed, and the completion must be ignored.
    pub(super) fn complete_url_elicitation(&self, elicitation_id: &str) -> bool {
        self.url_elicitations.lock().remove(elicitation_id)
    }

    /// Progress tokens this client is waiting on.
    pub(super) fn progress(&self) -> &ProgressTokens {
        &self.progress
    }

    /// Get the message dispatcher for handler registration
    ///
    /// This allows the Client to register request/notification handlers
    /// with the dispatcher.
    pub(super) fn dispatcher(&self) -> &Arc<MessageDispatcher> {
        &self.dispatcher
    }

    /// Send JSON-RPC request and await typed response
    ///
    /// The configured timeouts apply and a session expiry is reported as an
    /// ordinary transport error — which is what `initialize` needs, since it
    /// is the request that starts a session and must not try to renew one.
    /// Everything else goes through [`Self::request_with`].
    ///
    /// ## New Architecture (v2.0+)
    ///
    /// Instead of calling `transport.receive()` directly (which created the
    /// race condition), this method now:
    ///
    /// 1. Registers a oneshot channel with the dispatcher BEFORE sending
    /// 2. Sends the request via transport
    /// 3. Waits on the oneshot channel for the response
    ///
    /// The dispatcher's background task receives the response and routes it
    /// to the oneshot channel. This ensures responses always reach the right
    /// request() call, even when the server sends requests (elicitation, etc.)
    /// in between.
    ///
    /// ## Example Flow with Elicitation
    ///
    /// ```text
    /// Client: call_tool("test") → request(id=1)
    ///   1. Register oneshot channel for id=1
    ///   2. Send tools/call request
    ///   3. Wait on channel...
    ///
    /// Server: Sends elicitation/create request (id=2)
    ///   → Dispatcher routes to request handler
    ///   → Client processes elicitation
    ///   → Client sends elicitation response
    ///
    /// Server: Sends tools/call response (id=1)
    ///   → Dispatcher routes to oneshot channel for id=1
    ///   → request() receives response ✓
    /// ```
    pub(super) async fn request<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        self.request_with(method, params.as_ref(), &RequestOptions::default())
            .await
            .map_err(Error::from)
    }

    /// Send a JSON-RPC request with per-request options.
    ///
    /// However the request ends without a response — its timeout, the
    /// `total` cap, or the caller dropping the future — the server is told:
    /// with `notifications/cancelled`, or `tasks/cancel` for a
    /// task-augmented request. `initialize` is never cancelled.
    pub(super) async fn request_with<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<&serde_json::Value>,
        options: &RequestOptions,
    ) -> std::result::Result<R, RequestError> {
        let (timeout, cap) = options.deadline.resolve(&self.config.timeouts);
        let operation = self.request_inner(method, params, options, timeout, cap);

        let Some(total_timeout) = cap else {
            return operation.await;
        };
        // Dropping `operation` here is what cancels the request on the
        // server: its in-flight guard notices it never got an answer.
        match tokio::time::timeout(total_timeout, operation).await {
            Ok(result) => result,
            Err(_) => {
                let err = turbomcp_transport::TransportError::TotalTimeout {
                    operation: format!("{}()", method),
                    timeout: total_timeout,
                };
                Err(Error::transport(err.to_string()).into())
            }
        }
    }

    /// Inner request implementation without total timeout wrapper
    ///
    /// `timeout` is the per-request timeout; `cap` the `total` one wrapped
    /// around this future, used here only to bound what happens after it.
    async fn request_inner<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<&serde_json::Value>,
        options: &RequestOptions,
        timeout: Option<Duration>,
        cap: Option<Duration>,
    ) -> std::result::Result<R, RequestError> {
        // Generate unique request ID
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request_id = turbomcp_protocol::MessageId::from(id.to_string());

        // Tracked before the request goes out: a quick server can report
        // progress before `send` has even returned.
        let progress = options
            .progress_token
            .clone()
            .map(|token| self.progress.register(token))
            .transpose()?;
        let activity = progress.as_ref().map(ProgressRegistration::activity);

        let payload = serde_json::to_vec(&OutgoingRequest {
            jsonrpc: JsonRpcVersion,
            method,
            params,
            id: &request_id,
        })
        .map_err(|e| Error::internal(format!("Failed to serialize request: {e}")))?;

        // Step 1: Register oneshot channel BEFORE sending request via the
        // RAII guard so a mid-flight future drop can't leak the waiter map
        // entry (cancellation-safety).
        let (response_receiver, waiter_guard) = self
            .dispatcher
            .wait_for_response_guarded(request_id.clone());

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from(format!("req-{id}")),
            payload.into(),
        );

        // The guard cleans up the waiter if `send` errors out (drop fires
        // when we leave this scope).
        if let Err(e) = self.transport.send(message).await {
            let error = Error::transport(format!("Transport send failed: {e}"));
            return Err(match e {
                TransportError::SessionExpired(_) => RequestError::SessionExpired(error),
                _ => RequestError::Failed(error),
            });
        }

        // Step 2: on the wire. From here, leaving without a response means
        // telling the server to stop, which the guard does however we leave.
        let mut in_flight = InFlight {
            transport: Arc::clone(&self.transport),
            dispatcher: Arc::clone(&self.dispatcher),
            next_id: Arc::clone(&self.next_id),
            id: request_id,
            response: Some(response_receiver),
            waiter: Some(waiter_guard),
            // cancellation.mdx: "The `initialize` request MUST NOT be
            // cancelled by clients." A slow handshake — cold start, OAuth
            // discovery, a large index — is exactly when a timeout fires.
            on_abandon: if method == "initialize" {
                Abandon::Nothing
            } else if options.task_augmented {
                Abandon::CancelTask
            } else {
                Abandon::Notify
            },
            // The request has already outlived its own timeout, so that is no
            // measure of when the task will appear; the cap is the longest the
            // caller would ever have waited for it.
            chase_for: cap,
        };

        // Step 3: wait. A progress notification for this request restarts
        // the timeout (lifecycle.mdx allows it); the `total` cap around this
        // future still bounds the whole wait.
        let received = loop {
            let idle = async {
                match timeout {
                    Some(timeout) => tokio::time::sleep(timeout).await,
                    None => std::future::pending().await,
                }
            };
            let progressed = async {
                match &activity {
                    Some(activity) => activity.notified().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                response = in_flight.response() => break response,
                () = progressed => continue,
                () = idle => {
                    in_flight.abandon("client request timeout").await;
                    let err = turbomcp_transport::TransportError::RequestTimeout {
                        operation: format!("{}()", method),
                        timeout: timeout.unwrap_or_default(),
                    };
                    return Err(Error::transport(err.to_string()).into());
                }
            }
        };
        in_flight.complete();
        let response =
            received.map_err(|_| Error::transport("Response channel closed".to_string()))?;

        if let Some(progress) = progress {
            // The response overtook the queue: progress that arrived before
            // it may not have reached the handler yet. Deliver it first, so
            // no progress for this call is reported after the call returned.
            self.dispatcher.flush_notifications().await;
            // progress.mdx: a task's progress keeps using the token of the
            // request that created it, until the task is terminal.
            if options.task_augmented
                && let Some(task_id) = created_task_id(&response)
            {
                progress.keep_for_task(task_id.to_owned());
            }
        }

        // Handle JSON-RPC errors. `data` is kept: for some errors it is the
        // payload — a -32042 carries the URLs the user has to visit.
        if let Some(error) = response.error() {
            let mut err = Error::from_rpc_code(error.code, &error.message);
            if let Some(data) = &error.data {
                if error.code == turbomcp_protocol::types::URLElicitationRequiredError::ERROR_CODE {
                    self.track_elicitations_in(data);
                }
                err = err.with_data(data.clone());
            }
            return Err(err.into());
        }

        // Deserialize result
        serde_json::from_value(response.result().unwrap_or_default().clone())
            .map_err(|e| Error::internal(format!("Failed to deserialize response: {e}")).into())
    }

    /// Send a `notifications/cancelled` notification for the given request id,
    /// and stop waiting for its response.
    pub(super) async fn send_cancellation(
        &self,
        request_id: &turbomcp_protocol::MessageId,
        reason: Option<&str>,
    ) -> Result<()> {
        self.dispatcher.remove_response_waiter(request_id);
        send_cancelled(self.transport.as_ref(), request_id, reason).await
    }

    /// Send JSON-RPC notification (no response expected)
    pub(super) async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        send_notification(self.transport.as_ref(), method, params).await
    }

    /// Get transport reference
    ///
    /// Returns an Arc reference to the transport, allowing it to be shared
    /// with other components (like the message dispatcher).
    pub(super) fn transport(&self) -> &Arc<T> {
        &self.transport
    }
}

/// Send a JSON-RPC notification over `transport`.
async fn send_notification<T: Transport + ?Sized>(
    transport: &T,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<()> {
    // `params` is omitted rather than sent as `null`: JSON-RPC requires it
    // to be a structured value when present, and the TypeScript SDK's
    // schema rejects `"params": null` outright — which, for
    // `notifications/initialized`, fails the whole handshake.
    let mut request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
    });
    if let Some(params) = params {
        request["params"] = params;
    }

    let payload = serde_json::to_vec(&request)
        .map_err(|e| Error::internal(format!("Failed to serialize notification: {e}")))?;

    let message = TransportMessage::new(
        turbomcp_protocol::MessageId::from("notification"),
        payload.into(),
    );

    transport
        .send(message)
        .await
        .map_err(|e| Error::transport(format!("Transport send failed: {e}")))
}

/// Send `notifications/cancelled` for a request this client issued.
///
/// Per MCP §Cancellation, a client abandoning an in-flight request (timeout,
/// future drop, user cancellation) SHOULD send this so the server can stop
/// work. The `initialize` request MUST NOT be cancelled — callers gate that
/// themselves.
async fn send_cancelled<T: Transport + ?Sized>(
    transport: &T,
    request_id: &MessageId,
    reason: Option<&str>,
) -> Result<()> {
    let mut params = serde_json::Map::new();
    params.insert(
        "requestId".to_string(),
        serde_json::to_value(request_id)
            .map_err(|e| Error::internal(format!("Failed to serialize requestId: {e}")))?,
    );
    if let Some(reason) = reason {
        params.insert(
            "reason".to_string(),
            serde_json::Value::String(reason.into()),
        );
    }
    send_notification(
        transport,
        "notifications/cancelled",
        Some(serde_json::Value::Object(params)),
    )
    .await
}

/// [`send_cancelled`] for a request abandoned internally, where a failure to
/// send has nobody to report to: the request is abandoned locally either way.
async fn send_cancelled_best_effort<T: Transport + ?Sized>(
    transport: &T,
    request_id: &MessageId,
    reason: &str,
) {
    if let Err(e) = send_cancelled(transport, request_id, Some(reason)).await {
        tracing::debug!(request_id = ?request_id, "Could not send notifications/cancelled: {e}");
    }
}

/// The `taskId` of a `CreateTaskResult`, if `response` is one.
///
/// Decided by shape, the way a receiver that ignored the augmentation and
/// answered with a plain result has to be told apart from one that made a
/// task.
pub(super) fn created_task_id(response: &JsonRpcResponse) -> Option<&str> {
    response.result()?.get("task")?.get("taskId")?.as_str()
}

/// What an abandoned request owes the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Abandon {
    /// Nothing: the response arrived, or the request is `initialize`.
    Nothing,
    /// `notifications/cancelled` for the request id.
    Notify,
    /// `tasks/cancel` for the task the request creates, once its
    /// `CreateTaskResult` arrives. There is no task id to cancel until then,
    /// and `notifications/cancelled` is not allowed for these requests.
    CancelTask,
}

/// A request on the wire whose response has not been received.
///
/// If this is dropped before [`Self::complete`] — the caller's future was
/// dropped, or the `total` cap fired around it — the server is told the
/// request has been abandoned, as it is when the request times out. Before,
/// only the per-request timeout did that, and a server kept working on
/// everything else a client gave up on.
struct InFlight<T: Transport + 'static> {
    transport: Arc<T>,
    dispatcher: Arc<MessageDispatcher>,
    next_id: Arc<AtomicU64>,
    id: MessageId,
    response: Option<oneshot::Receiver<JsonRpcResponse>>,
    waiter: Option<WaiterGuard>,
    on_abandon: Abandon,
    /// How long to keep waiting for the `CreateTaskResult` of an abandoned
    /// task-augmented request; `None` waits until the dispatcher shuts down.
    chase_for: Option<Duration>,
}

impl<T: Transport + 'static> InFlight<T> {
    async fn response(
        &mut self,
    ) -> std::result::Result<JsonRpcResponse, oneshot::error::RecvError> {
        match self.response.as_mut() {
            Some(response) => response.await,
            None => std::future::pending().await,
        }
    }

    /// The response arrived (or never can): nothing is owed.
    fn complete(mut self) {
        self.on_abandon = Abandon::Nothing;
        if let Some(waiter) = self.waiter.take() {
            waiter.disarm();
        }
    }

    /// Give up on the request now, telling the server before returning.
    async fn abandon(mut self, reason: &str) {
        if self.on_abandon == Abandon::Notify {
            self.on_abandon = Abandon::Nothing;
            self.waiter.take();
            send_cancelled_best_effort(self.transport.as_ref(), &self.id, reason).await;
        }
        // `CancelTask` has to wait for the task to exist; Drop hands that
        // off to a background task either way.
    }
}

impl<T: Transport + 'static> Drop for InFlight<T> {
    fn drop(&mut self) {
        let on_abandon = std::mem::replace(&mut self.on_abandon, Abandon::Nothing);
        if on_abandon == Abandon::Nothing {
            return;
        }
        // Drop cannot await. Without a runtime to spawn on there is no way
        // to send anything, and the waiter guard still cleans up.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let transport = Arc::clone(&self.transport);
        let id = self.id.clone();
        match on_abandon {
            Abandon::Notify => {
                self.waiter.take();
                runtime.spawn(async move {
                    send_cancelled_best_effort(
                        transport.as_ref(),
                        &id,
                        "request abandoned by the client",
                    )
                    .await;
                });
            }
            Abandon::CancelTask => {
                let (Some(response), Some(waiter)) = (self.response.take(), self.waiter.take())
                else {
                    return;
                };
                runtime.spawn(cancel_created_task(
                    transport,
                    Arc::clone(&self.dispatcher),
                    Arc::clone(&self.next_id),
                    response,
                    waiter,
                    self.chase_for,
                ));
            }
            Abandon::Nothing => {}
        }
    }
}

/// Cancel the task an abandoned task-augmented request creates.
///
/// cancellation.mdx requires `tasks/cancel` for these, and it needs the task
/// id, which only the `CreateTaskResult` carries — so this waits for the
/// response the caller gave up on. A plain result instead means the server
/// ran the request inline and there is nothing left to cancel.
async fn cancel_created_task<T: Transport + 'static>(
    transport: Arc<T>,
    dispatcher: Arc<MessageDispatcher>,
    next_id: Arc<AtomicU64>,
    response: oneshot::Receiver<JsonRpcResponse>,
    waiter: WaiterGuard,
    wait_at_most: Option<Duration>,
) {
    let response = match wait_at_most {
        Some(limit) => tokio::time::timeout(limit, response).await.ok(),
        None => Some(response.await),
    };
    drop(waiter);
    let Some(Ok(response)) = response else {
        return;
    };
    let Some(task_id) = created_task_id(&response) else {
        return;
    };

    let id = MessageId::from(next_id.fetch_add(1, Ordering::Relaxed).to_string());
    let params = serde_json::json!({ "taskId": task_id });
    let Ok(payload) = serde_json::to_vec(&OutgoingRequest {
        jsonrpc: JsonRpcVersion,
        method: "tasks/cancel",
        params: Some(&params),
        id: &id,
    }) else {
        return;
    };
    // Waited on only so the answer is not logged as a response to nothing.
    let (answer, _waiter) = dispatcher.wait_for_response_guarded(id.clone());
    let sent = transport
        .send(TransportMessage::new(id, payload.into()))
        .await;
    match sent {
        Ok(()) => {
            tracing::debug!(task_id, "Cancelled the task of an abandoned request");
            match wait_at_most {
                Some(limit) => drop(tokio::time::timeout(limit, answer).await),
                None => drop(answer.await),
            }
        }
        Err(e) => tracing::debug!(task_id, "Could not send tasks/cancel: {e}"),
    }
}

/// Progress tokens this client is waiting on.
///
/// progress.mdx: tokens "MUST be unique across all active requests", and
/// senders "SHOULD track active progress tokens". Tracking is what lets the
/// client ignore a notification for a token it never issued or has finished
/// with, and restart a request's timeout when the server shows it is still
/// working on it.
#[derive(Debug, Default)]
pub(super) struct ProgressTokens {
    state: Arc<Mutex<ProgressState>>,
}

#[derive(Debug, Default)]
struct ProgressState {
    /// Every live token, with the signal that restarts its request's timeout.
    active: HashMap<ProgressToken, Arc<Notify>>,
    /// Tokens kept live past their request for the task it created.
    tasks: HashMap<String, ProgressToken>,
}

impl ProgressTokens {
    /// Start tracking `token` for a request about to be sent.
    fn register(&self, token: ProgressToken) -> Result<ProgressRegistration> {
        let activity = Arc::new(Notify::new());
        let mut state = self.state.lock();
        if state.active.contains_key(&token) {
            return Err(Error::invalid_request(format!(
                "progress token {token} is already in use by another request; \
                 tokens must be unique across active requests"
            )));
        }
        state.active.insert(token.clone(), Arc::clone(&activity));
        Ok(ProgressRegistration {
            state: Arc::clone(&self.state),
            token,
            activity,
            kept: false,
        })
    }

    /// Note a progress notification for `token`.
    ///
    /// Returns `false` for a token that is not live — never issued, or its
    /// request (or task) already finished — which the caller should ignore.
    pub(super) fn record(&self, token: &ProgressToken) -> bool {
        match self.state.lock().active.get(token) {
            Some(activity) => {
                activity.notify_one();
                true
            }
            None => false,
        }
    }

    /// The task `task_id` reached a terminal status; its token is finished.
    pub(super) fn finish_task(&self, task_id: &str) {
        let mut state = self.state.lock();
        if let Some(token) = state.tasks.remove(task_id) {
            state.active.remove(&token);
        }
    }
}

/// A progress token tracked for one request; released when dropped.
struct ProgressRegistration {
    state: Arc<Mutex<ProgressState>>,
    token: ProgressToken,
    activity: Arc<Notify>,
    kept: bool,
}

impl ProgressRegistration {
    fn activity(&self) -> Arc<Notify> {
        Arc::clone(&self.activity)
    }

    /// Keep the token live for the task the request created, until
    /// [`ProgressTokens::finish_task`].
    fn keep_for_task(mut self, task_id: String) {
        self.state.lock().tasks.insert(task_id, self.token.clone());
        self.kept = true;
    }
}

impl Drop for ProgressRegistration {
    fn drop(&mut self) {
        if !self.kept {
            self.state.lock().active.remove(&self.token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use turbomcp_transport::{
        TransportCapabilities, TransportConfig, TransportError, TransportMetrics, TransportResult,
        TransportState, TransportType,
    };

    #[derive(Debug)]
    struct MockTransport {
        capabilities: TransportCapabilities,
        fail_send: AtomicBool,
    }

    impl MockTransport {
        fn ok() -> Self {
            Self {
                capabilities: TransportCapabilities::default(),
                fail_send: AtomicBool::new(false),
            }
        }

        fn fail_send() -> Self {
            Self {
                capabilities: TransportCapabilities::default(),
                fail_send: AtomicBool::new(true),
            }
        }
    }

    impl Transport for MockTransport {
        fn transport_type(&self) -> TransportType {
            TransportType::Stdio
        }

        fn capabilities(&self) -> &TransportCapabilities {
            &self.capabilities
        }

        fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
            Box::pin(async { TransportState::Connected })
        }

        fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn send(
            &self,
            _message: TransportMessage,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            let fail = self.fail_send.load(Ordering::Relaxed);
            Box::pin(async move {
                if fail {
                    Err(TransportError::SendFailed("send failed".to_string()))
                } else {
                    Ok(())
                }
            })
        }

        fn receive(
            &self,
        ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>>
        {
            Box::pin(async { Ok(None) })
        }

        fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
            Box::pin(async { TransportMetrics::default() })
        }

        fn configure(
            &self,
            _config: TransportConfig,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn test_request_timeout_cleans_up_waiter() {
        let config = TransportConfig {
            timeouts: turbomcp_transport::config::TimeoutConfig {
                request: Some(Duration::from_millis(10)),
                total: Some(Duration::from_millis(25)),
                ..Default::default()
            },
            ..Default::default()
        };
        let client = ProtocolClient::with_config(MockTransport::ok(), config);

        let result: Result<serde_json::Value> = client.request("tools/list", None).await;
        assert!(result.is_err());
        assert_eq!(client.dispatcher.response_waiter_count(), 0);

        client.dispatcher.shutdown();
    }

    #[tokio::test]
    async fn test_send_failure_cleans_up_waiter() {
        let client =
            ProtocolClient::with_config(MockTransport::fail_send(), TransportConfig::default());

        let result: Result<serde_json::Value> = client.request("tools/list", None).await;
        assert!(result.is_err());
        assert_eq!(client.dispatcher.response_waiter_count(), 0);

        client.dispatcher.shutdown();
    }
}
