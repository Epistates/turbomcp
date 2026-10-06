//! The client connection actor and the [`Connection`] handle.
//!
//! ## Concurrency model — the inverted serve driver
//!
//! A client is the mirror image of the server's `serve` driver
//! (`turbomcp_service::serve_with`): one task owns the [`Transport`], and a
//! single [`tokio::select!`] loop multiplexes the two directions over the one
//! `&mut self` channel. The borrows never overlap because only *one* transport
//! future is ever a selected branch:
//!
//! - **Outbound:** the [`Connection`] handle pushes frames (requests,
//!   notifications, replies to server→client requests) onto an `mpsc`; the
//!   loop's `outbound.recv()` arm hands each to `transport.send()` — `send` runs
//!   in the arm *body*, after a non-transport future fired, so it doesn't hold a
//!   borrow across the select.
//! - **Inbound:** `transport.recv()` *is* a selected future. A `Response` is
//!   matched to its waiting request via the [`Pending`] table; a `Notification`
//!   invalidates whatever it obsoletes in the [`ResponseCache`] and then reaches
//!   the registered [`NotificationHandler`]; a server→client `Request` goes to
//!   whichever handler serves that method (elicit/sample/roots) on a spawned
//!   task whose reply is sent back through a [`WeakSender`](mpsc::WeakSender)
//!   — or, with none registered for it, answered `-32601` inline.
//!
//! A request that is abandoned rather than answered — the timeout in
//! [`Connection::request`] firing, or its caller dropping the future — leaves
//! the `Pending` table *and* the server's queue: see [`AbandonGuard`].
//!
//! The actor holds **no strong** outbound `Sender` (only a [`WeakSender`] for
//! replies), so when every [`Connection`] handle drops, the channel closes, the
//! `outbound.recv()` arm yields `None`, and the loop exits — closing the
//! transport and failing any still-waiting requests with [`ClientError::Closed`].

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{AbortHandle, JoinSet};
use turbomcp_core::{
    Extensions, InvalidFrame, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, ProtocolVersion, RequestId, meta,
};
use turbomcp_protocol::methods::{notification, request};
use turbomcp_service::Transport;

use crate::cache::ResponseCache;
use crate::error::{ClientError, ClientResult};
use crate::handler::{ClientHandlers, dispatch_server_request};
use crate::input::{Answerer, InputRoutes, RouteKey};
use crate::progress::ProgressRoutes;
use crate::subscription::SubscriptionRoutes;

/// Default per-request timeout — a request with no answer in this window fails
/// with [`ClientError::Timeout`] rather than hanging forever.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The waiting side of in-flight requests: id → the oneshot its caller awaits.
type Pending = Mutex<HashMap<RequestId, oneshot::Sender<Result<Value, ClientError>>>>;

/// One outbound frame and the facts meant for the transport alone (the
/// revision it goes out under, `Mcp-Param-*` mirrors).
type Frame = (JsonRpcMessage, Extensions);

/// Shared connection state, held by every [`Connection`] clone.
struct Inner {
    /// Per-call input handlers (shared with the actor).
    input: Arc<InputRoutes>,
    /// Frames the client wants to send (the actor owns the receiver). The actor
    /// holds only a `WeakSender`, so dropping all handles closes the channel.
    outbound: mpsc::Sender<Frame>,
    /// In-flight requests awaiting a response (shared with the actor).
    pending: Arc<Pending>,
    /// Monotonic request-id source (process-local; integer ids).
    next_id: AtomicI64,
    /// How long [`Connection::request`] waits before giving up.
    request_timeout: Duration,
    /// The revision this session settled on, shared with the actor.
    ///
    /// The actor starts before the handshake does, so this is written once the
    /// negotiation resolves. It decides how a *reply* to a server→client
    /// request is shaped, which differs by revision — `2025-06-18` sampling
    /// results carry one content block, not a list.
    negotiated: Arc<Mutex<ProtocolVersion>>,
    shutdown: tokio_util::sync::CancellationToken,
    done: tokio_util::sync::CancellationToken,
    admission: tokio::sync::Semaphore,
    /// Whether the transport is Streamable HTTP, where header-level features
    /// (`x-mcp-header` mirrors, `MCP-Protocol-Version`) apply.
    carries_headers: bool,
    /// Where each call's `notifications/progress` goes (shared with the actor).
    progress: Arc<ProgressRoutes>,
    /// Live `subscriptions/listen` streams (shared with the actor).
    subscriptions: Arc<SubscriptionRoutes>,
    /// How many server→client requests this client is answering right now.
    /// A request's timeout stops while any are: the client, not the server,
    /// is the one taking its time.
    inbound: watch::Receiver<usize>,
    /// Watches every request this connection sends.
    observer: Option<Arc<dyn crate::RequestObserver>>,
    /// The connection the transport carries, for the observer.
    network: Option<turbomcp_service::NetworkFacts>,
    /// When the connection opened, for the observer's session duration.
    opened: Instant,
    /// Whether a handshake settled `negotiated` (it starts as a default).
    settled: AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(observer) = &self.observer {
            let version = self
                .negotiated
                .lock()
                .map(|v| v.clone())
                .unwrap_or(ProtocolVersion::LATEST);
            observer.closed(&crate::observe::ClosedSession {
                duration: self.opened.elapsed(),
                protocol_version: self.settled.load(Ordering::Acquire).then_some(&version),
                network: self.network.as_ref(),
            });
        }
    }
}

/// A raw connection to a live MCP peer — the transport + request/response
/// correlation, with no protocol knowledge.
///
/// Cheaply [`Clone`]able (all clones share one connection); dropping the last
/// clone closes the connection. The [`request`](Self::request) /
/// [`notify`](Self::notify) methods speak raw JSON-RPC. The typed, negotiated
/// MCP API (`initialize`, `list_tools`, …) is [`Client`](crate::Client), which
/// wraps a `Connection` and stamps the right version metadata.
#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

impl core::fmt::Debug for Connection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Connection")
            .field("request_timeout", &self.inner.request_timeout)
            .field("closed", &self.inner.outbound.is_closed())
            .field(
                "in_flight",
                &self.inner.pending.lock().map(|p| p.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl Connection {
    /// Spawn the connection actor over `transport` with the default timeout and
    /// no client-serving handler.
    pub fn new<T>(transport: T) -> Self
    where
        T: Transport,
    {
        Self::connect(
            transport,
            DEFAULT_REQUEST_TIMEOUT,
            ClientHandlers::default(),
        )
    }

    /// Spawn the connection actor with an explicit per-request timeout and no
    /// client-serving handler.
    pub fn with_timeout<T>(transport: T, request_timeout: Duration) -> Self
    where
        T: Transport,
    {
        Self::connect(transport, request_timeout, ClientHandlers::default())
    }

    /// Spawn the connection actor with a timeout and an optional
    /// [`ClientHandlers`] set for server→client requests (elicit/sample/roots).
    pub fn connect<T>(transport: T, request_timeout: Duration, handler: ClientHandlers) -> Self
    where
        T: Transport,
    {
        Self::connect_with_cache(transport, request_timeout, handler, None, None)
    }

    /// [`connect`](Self::connect), plus a [`ResponseCache`] the actor
    /// invalidates on inbound `*_list_changed` / `resources/updated`
    /// notifications (the [`Client`](crate::Client) wires this).
    pub(crate) fn connect_with_cache<T>(
        transport: T,
        request_timeout: Duration,
        handler: ClientHandlers,
        cache: Option<Arc<ResponseCache>>,
        observer: Option<Arc<dyn crate::RequestObserver>>,
    ) -> Self
    where
        T: Transport,
    {
        // Capacity mirrors the server driver's default outbound buffer.
        let (tx, rx) = mpsc::channel::<Frame>(1024);
        let pending: Arc<Pending> = Arc::new(Mutex::new(HashMap::new()));
        let weak_out = tx.downgrade();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let done = tokio_util::sync::CancellationToken::new();
        let negotiated = Arc::new(Mutex::new(ProtocolVersion::LATEST));
        let carries_headers = transport.carries_headers();
        let network = transport.network();
        let progress = Arc::new(ProgressRoutes::default());
        let subscriptions = Arc::new(SubscriptionRoutes::default());
        let (inbound_tx, inbound) = watch::channel(0);
        let input = Arc::new(InputRoutes::default());
        tokio::spawn(actor(
            transport,
            rx,
            SessionState {
                pending: Arc::clone(&pending),
                handler,
                weak_out,
                cache,
                negotiated: Arc::clone(&negotiated),
                progress: Arc::clone(&progress),
                subscriptions: Arc::clone(&subscriptions),
                inbound: inbound_tx,
                input: Arc::clone(&input),
            },
            (shutdown.clone(), done.clone()),
        ));
        Self {
            inner: Arc::new(Inner {
                outbound: tx,
                pending,
                next_id: AtomicI64::new(1),
                request_timeout,
                negotiated,
                shutdown,
                done,
                admission: tokio::sync::Semaphore::new(1024),
                carries_headers,
                progress,
                subscriptions,
                inbound,
                input,
                observer,
                network,
                opened: Instant::now(),
                settled: AtomicBool::new(false),
            }),
        }
    }

    /// Route the input requests of task `task_id` to `handlers` until the
    /// guard drops.
    pub(crate) fn route_task_input(
        &self,
        task_id: &str,
        handlers: ClientHandlers,
    ) -> crate::input::InputGuard {
        self.inner
            .input
            .register(RouteKey::Task(task_id.to_owned()), handlers)
    }

    /// Record the revision the handshake settled on, so replies to
    /// server→client requests are shaped for it.
    pub(crate) fn set_negotiated_version(&self, version: ProtocolVersion) {
        *self
            .inner
            .negotiated
            .lock()
            .expect("negotiated version mutex poisoned") = version;
        self.inner.settled.store(true, Ordering::Release);
    }

    /// Whether this connection runs over Streamable HTTP, which is where the
    /// spec's HTTP-only rules (`x-mcp-header` mirroring) apply.
    pub(crate) fn carries_headers(&self) -> bool {
        self.inner.carries_headers
    }

    /// Cancel this connection and wait for its owned tasks and transport to
    /// close. Applies to all clones; transport cleanup is bounded to five seconds.
    pub async fn close(&self) {
        self.inner.shutdown.cancel();
        self.inner.done.cancelled().await;
    }

    /// Issue a request and await its result.
    ///
    /// # Errors
    /// [`ClientError::Rpc`] if the server returns an error object,
    /// [`ClientError::Timeout`] if no answer arrives in time, or
    /// [`ClientError::Closed`] if the connection is gone.
    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> ClientResult<Value> {
        self.request_with(method, params, Extensions::new()).await
    }

    /// [`request`](Self::request), with facts for the transport: the
    /// revision the request goes out under
    /// ([`WireVersion`](turbomcp_service::WireVersion)) and any `Mcp-Param-*`
    /// mirrors ([`ParamHeaders`](turbomcp_service::ParamHeaders)).
    ///
    /// # Errors
    /// As [`request`](Self::request).
    pub async fn request_with(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
        facts: Extensions,
    ) -> ClientResult<Value> {
        self.request_waiting(method, params, facts, Wait::default())
            .await
    }

    /// Per-call progress routes, for a caller that mints a token.
    pub(crate) fn progress_routes(&self) -> &Arc<ProgressRoutes> {
        &self.inner.progress
    }

    /// Live subscription routes.
    pub(crate) fn subscriptions(&self) -> &Arc<SubscriptionRoutes> {
        &self.inner.subscriptions
    }

    /// A fresh request id, for a caller that has to know it before the
    /// request goes out (a subscription, whose id this is).
    pub(crate) fn mint_id(&self) -> RequestId {
        RequestId::Number(self.inner.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// [`request_with`](Self::request_with), waiting as `wait` says.
    ///
    /// The clock stops while this client is answering a server→client request
    /// (a legacy `tools/call` whose server asked the user something): the
    /// protocol can't say which call such a request belongs to, and timing out
    /// the call while its user types the answer threw the answer away.
    pub(crate) async fn request_waiting(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
        facts: Extensions,
        wait: Wait,
    ) -> ClientResult<Value> {
        self.request_as(self.mint_id(), method, params, facts, wait)
            .await
    }

    /// [`request_waiting`](Self::request_waiting) under an id from
    /// [`mint_id`](Self::mint_id), shown to the observer if there is one.
    pub(crate) async fn request_as(
        &self,
        id: RequestId,
        method: impl Into<String>,
        params: Option<Value>,
        facts: Extensions,
        wait: Wait,
    ) -> ClientResult<Value> {
        let method = method.into();
        let Some(observer) = self.inner.observer.clone() else {
            return self.request_as_inner(id, method, params, facts, wait).await;
        };
        let mut params = params;
        let scope = observer.start(&crate::OutboundRequest {
            id: &id,
            method: &method,
            params: params.as_ref().and_then(Value::as_object),
            protocol_version: facts.get::<turbomcp_service::WireVersion>().map(|w| &w.0),
            network: self.inner.network.as_ref(),
        });
        crate::observe::merge_meta(&mut params, scope.meta());
        let result = self.request_as_inner(id, method, params, facts, wait).await;
        scope.finish(result.as_ref());
        result
    }

    async fn request_as_inner(
        &self,
        id: RequestId,
        method: String,
        params: Option<Value>,
        facts: Extensions,
        wait: Wait,
    ) -> ClientResult<Value> {
        let timeout = wait.timeout.unwrap_or(self.inner.request_timeout);
        let mut deadline = tokio::time::Instant::now() + timeout;
        let _admission = tokio::time::timeout_at(deadline, self.inner.admission.acquire())
            .await
            .map_err(|_| ClientError::Timeout)?
            .map_err(|_| ClientError::Closed)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        // Routed before the request goes out: its server requests may arrive
        // before anything else does.
        let _input = wait.input.clone().map(|handlers| {
            self.inner
                .input
                .register(RouteKey::Request(id.clone()), handlers)
        });
        self.inner
            .pending
            .lock()
            .expect("pending mutex poisoned")
            .insert(id.clone(), reply_tx);

        // The same deadline covers admission and the response. Register cleanup
        // before the first await, including a caller dropping an unsent request.
        let mut abandon = AbandonGuard {
            conn: self,
            id: id.clone(),
            reason: Some("the caller dropped the request"),
            notify: false,
        };
        let notify_on_abandon = cancellable(&method, params.as_ref());
        let msg = JsonRpcMessage::Request(JsonRpcRequest::new(id.clone(), method, params));
        match tokio::time::timeout_at(deadline, self.inner.outbound.send((msg, facts))).await {
            Ok(Ok(())) => abandon.notify = notify_on_abandon,
            Ok(Err(_)) => return Err(ClientError::Closed),
            Err(_) => return Err(ClientError::Timeout),
        }

        let reply = |r: Result<ClientResult<Value>, oneshot::error::RecvError>| match r {
            Ok(result) => result,
            // The actor dropped the sender (connection closed) before replying.
            Err(_recv) => Err(ClientError::Closed),
        };
        let mut reply_rx = reply_rx;
        let mut inbound = self.inner.inbound.clone();
        let mut ticks = wait.progress;
        let outcome = loop {
            tokio::select! {
                r = &mut reply_rx => break reply(r),
                () = tokio::time::sleep_until(deadline) => {
                    if *inbound.borrow() > 0 {
                        // Paused: wait for the client to finish answering,
                        // then give the request its full timeout again.
                        tokio::select! {
                            r = &mut reply_rx => break reply(r),
                            _ = inbound.wait_for(|n| *n == 0) => {
                                deadline = tokio::time::Instant::now() + timeout;
                                continue;
                            }
                        }
                    }
                    abandon.reason = Some("the client's request timeout elapsed");
                    return Err(ClientError::Timeout);
                }
                Ok(()) = async {
                    match ticks.as_mut() {
                        Some(ticks) => ticks.changed().await,
                        None => std::future::pending().await,
                    }
                } => {
                    deadline = tokio::time::Instant::now() + timeout;
                }
            }
        };
        // Answered: `complete_pending` already took the entry, and there is
        // nothing for the server to stop doing.
        abandon.reason = None;
        outcome
    }

    /// Send a fire-and-forget notification (no response is expected).
    ///
    /// # Errors
    /// [`ClientError::Closed`] if the connection is gone.
    pub async fn notify(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> ClientResult<()> {
        self.notify_with(method, params, Extensions::new()).await
    }

    /// [`notify`](Self::notify), with facts for the transport (see
    /// [`request_with`](Self::request_with)).
    ///
    /// # Errors
    /// [`ClientError::Closed`] if the connection is gone.
    pub async fn notify_with(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
        facts: Extensions,
    ) -> ClientResult<()> {
        use turbomcp_core::JsonRpcNotification;
        let msg = JsonRpcMessage::Notification(JsonRpcNotification::new(method, params));
        self.inner
            .outbound
            .send((msg, facts))
            .await
            .map_err(|_| ClientError::Closed)
    }

    /// Push a raw frame onto the outbound wire (e.g. a reply to a server→client
    /// request). Ordered with all other outbound frames by the single writer.
    ///
    /// # Errors
    /// [`ClientError::Closed`] if the connection is gone.
    pub async fn send_message(&self, msg: JsonRpcMessage) -> ClientResult<()> {
        self.inner
            .outbound
            .send((msg, Extensions::new()))
            .await
            .map_err(|_| ClientError::Closed)
    }

    /// Drop a pending request that will never complete (timed out, or never
    /// reached the wire).
    fn forget(&self, id: &RequestId) {
        self.inner
            .pending
            .lock()
            .expect("pending mutex poisoned")
            .remove(id);
    }

    /// Tell the server to stop working on `id` (cancellation spec: receivers
    /// SHOULD stop processing, free resources, and send no response).
    ///
    /// This runs from a `Drop`, which cannot await, so the fast path is
    /// `try_send` preserves ordering with the request. If its queue is full,
    /// cancel the connection instead of spawning an unbounded detached waiter.
    pub(crate) fn cancel_on_wire(&self, id: &RequestId, reason: &str) {
        let params = serde_json::json!({ "requestId": id, "reason": reason });
        let msg = JsonRpcMessage::Notification(turbomcp_core::JsonRpcNotification::new(
            notification::CANCELLED,
            Some(params),
        ));
        match self.inner.outbound.try_send((msg, Extensions::new())) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                // No detached waiters during cancellation storms. Closing the
                // overloaded connection cancels its transport-owned requests.
                self.inner.shutdown.cancel();
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::debug!(request_id = ?id, "connection closed; nothing to cancel");
            }
        }
    }
}

/// May a client abandoning `method` say so with `notifications/cancelled`?
///
/// Two requests are carved out by the cancellation spec, and both are MUST NOT
/// rather than a preference.
fn cancellable(method: &str, params: Option<&Value>) -> bool {
    // "The `initialize` request MUST NOT be cancelled by clients." Its draft
    // counterpart is a plain request with no session to unwind, so cancelling
    // it says nothing the disconnect does not.
    if method == request::INITIALIZE || method == request::DISCOVER {
        return false;
    }
    // "For task-augmented requests, the `tasks/cancel` request MUST be used
    // instead of the `notifications/cancelled` notification." The `task` field
    // is what augments the request, so its presence is the test. A draft task
    // is server-initiated and has no such field: until the server hands back a
    // handle there is no task to cancel, and the notification is all we have.
    !params.is_some_and(|p| p.get("task").is_some())
}

/// Retracts an abandoned request unless disarmed, covering both ways a caller
/// stops waiting: our own request timeout, and the future being dropped
/// mid-flight. Either way the entry has to leave `pending` (or a long-lived
/// client leaks one per abandoned call) and the server has to be told, or it
/// keeps working on an answer no one will read.
struct AbandonGuard<'a> {
    conn: &'a Connection,
    id: RequestId,
    /// Why the request was abandoned; `None` once it resolved, which disarms.
    reason: Option<&'static str>,
    /// Cleared for the requests [`cancellable`] rules out. The pending entry is
    /// still retracted; only the notification is withheld.
    notify: bool,
}

impl Drop for AbandonGuard<'_> {
    fn drop(&mut self) {
        let Some(reason) = self.reason else { return };
        self.conn.forget(&self.id);
        if self.notify {
            self.conn.cancel_on_wire(&self.id, reason);
        }
    }
}

/// How long one request waits, when it isn't the connection's default.
#[derive(Clone, Debug, Default)]
pub(crate) struct Wait {
    /// Instead of the connection's request timeout.
    pub(crate) timeout: Option<Duration>,
    /// Ticks on each progress update; each restarts the timeout.
    pub(crate) progress: Option<watch::Receiver<()>>,
    /// Who answers the server requests that belong to this one.
    pub(crate) input: Option<ClientHandlers>,
}

/// The connection actor: owns the transport, multiplexes both directions.
/// Everything the actor and its inbound router need beyond the transport
/// itself: what a frame might complete, who answers it, and how to reply.
struct SessionState {
    /// In-flight requests awaiting a response.
    pending: Arc<Pending>,
    /// How this client answers server→client requests.
    handler: ClientHandlers,
    /// Where a spawned handler task writes its reply. Weak so that dropping
    /// every `Connection` still closes the channel.
    weak_out: mpsc::WeakSender<Frame>,
    /// The SEP-2549 response cache, invalidated on inbound notifications.
    cache: Option<Arc<ResponseCache>>,
    /// The revision the handshake settled on: written once, read per inbound
    /// server→client request to shape the reply.
    negotiated: Arc<Mutex<ProtocolVersion>>,
    /// Per-call progress routes.
    progress: Arc<ProgressRoutes>,
    /// Live subscriptions.
    subscriptions: Arc<SubscriptionRoutes>,
    /// Published count of server→client requests in their handlers.
    inbound: watch::Sender<usize>,
    /// Per-call input handlers.
    input: Arc<InputRoutes>,
}

async fn actor<T>(
    mut transport: T,
    mut outbound: mpsc::Receiver<Frame>,
    state: SessionState,
    lifecycle: (
        tokio_util::sync::CancellationToken,
        tokio_util::sync::CancellationToken,
    ),
) where
    T: Transport,
{
    let (shutdown, done) = lifecycle;
    let _done = done.drop_guard();
    let mut dispatch = Dispatch::new(&state.handler, transport.carries_headers());
    loop {
        let answering = dispatch.inflight.len();
        state.inbound.send_if_modified(|n| {
            let changed = *n != answering;
            *n = answering;
            changed
        });
        tokio::select! {
            () = shutdown.cancelled() => break,
            Some(finished) = dispatch.requests.join_next(), if !dispatch.requests.is_empty() => {
                // An aborted task (the server cancelled it) was already
                // forgotten when it was aborted.
                if let Ok(id) = finished {
                    dispatch.inflight.remove(&id);
                }
            },
            // Outbound: a frame to put on the wire.
            out = outbound.recv() => {
                match out {
                    // Cancellation abandons this frame even though it has
                    // already left the channel. That is the opposite of the
                    // server driver, which owes callers a drain window and so
                    // must finish a write it has started — deliberately, and
                    // not an oversight: `close()` is documented as "cancel this
                    // connection", so it has to return promptly rather than
                    // wait out a stalled peer for the write timeout below.
                    Some((msg, facts)) => {
                        tokio::select! {
                            () = shutdown.cancelled() => break,
                            result = tokio::time::timeout(Duration::from_secs(30), transport.send_with(msg, facts)) => {
                                if !matches!(result, Ok(Ok(()))) { break; }
                            }
                        }
                    }
                    // All Connection handles dropped — nothing more to send.
                    None => break,
                }
            }
            // Inbound: the next frame from the server.
            frame = transport.recv_with() => {
                match frame {
                    Ok(Some((msg, facts))) => {
                        let related = facts
                            .get::<turbomcp_service::RelatedRequest>()
                            .map(|r| r.0.clone());
                        let failure = match &msg {
                            JsonRpcMessage::Response(r) => r
                                .id
                                .as_ref()
                                .and_then(|id| transport.take_failure(id)),
                            _ => None,
                        };
                        if let Some(reply) =
                            route_inbound(msg, failure, related.as_ref(), &state, &mut dispatch)
                        {
                            tokio::select! {
                                () = shutdown.cancelled() => break,
                                result = tokio::time::timeout(Duration::from_secs(30), transport.send(reply)) => {
                                    if !matches!(result, Ok(Ok(()))) { break; }
                                }
                            }
                        }
                    }
                    Ok(None) => break, // clean EOF
                    Err(e) => match T::invalid_frame(e) {
                        Ok(bad) => {
                            if let Some(reply) = invalid_from_server(&bad, &state.pending) {
                                tokio::select! {
                                    () = shutdown.cancelled() => break,
                                    result = tokio::time::timeout(Duration::from_secs(30), transport.send(reply)) => {
                                        if !matches!(result, Ok(Ok(()))) { break; }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "client transport recv failed; closing");
                            break;
                        }
                    },
                }
            }
        }
    }

    // Connection is down: drop every waiting oneshot sender. A caller blocked in
    // `request` sees its receiver close and returns `ClientError::Closed`.
    // This happens *before* the close below, which can block on I/O — a caller
    // must learn the connection died promptly, not after a shutdown round trip.
    // No new request may be accepted after pending calls are drained. Keeping
    // this receiver open during transport.close allowed a late registration
    // to enqueue successfully after the only pending cleanup had already run.
    outbound.close();
    state
        .pending
        .lock()
        .expect("pending mutex poisoned")
        .clear();

    // Shut the transport down deliberately rather than by drop. Each transport
    // owes the peer something on the way out that dropping a socket does not
    // deliver: HTTP sends the spec's `DELETE` to terminate its session (absent
    // it, a long-lived server accumulates one session per client that ever
    // connected), and WebSocket sends its Close frame. Best-effort — the
    // connection is already going away, so a failure here has no one to tell.
    dispatch.requests.abort_all();
    while dispatch.requests.join_next().await.is_some() {}
    state.subscriptions.lose_all();
    if let Some(notifier) = dispatch.notifier.take() {
        notifier.abort();
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), transport.close()).await;
}

/// How many server→client requests may be in handlers at once. Past it a
/// request is answered `-32603` at once, rather than the connection closing.
const MAX_INBOUND_REQUESTS: usize = 64;

/// How many notifications may wait for the handler. Past it one is dropped
/// with a warning, rather than the connection closing.
const NOTIFICATION_QUEUE: usize = 1024;

/// Where inbound work goes once the actor has read it.
struct Dispatch {
    /// Notifications, in arrival order, for the one task that delivers them.
    /// Each used to be its own task, so progress could reach the handler at
    /// 90% before 10%, and a burst past 128 closed the connection.
    notes: Option<mpsc::Sender<(JsonRpcNotification, Option<String>)>>,
    notifier: Option<tokio::task::JoinHandle<()>>,
    /// Server→client requests in their handlers, each task yielding its id.
    requests: JoinSet<RequestId>,
    /// The same requests by id, so a `notifications/cancelled` can stop one.
    inflight: HashMap<RequestId, AbortHandle>,
    /// Whether the transport is Streamable HTTP, where on 2026-07-28 "The
    /// client MUST NOT send JSON-RPC responses".
    http: bool,
}

impl Dispatch {
    fn new(handler: &ClientHandlers, http: bool) -> Self {
        let (notes, notifier) = if handler.elicitation.is_some()
            || handler.notifications.is_some()
            || !handler.extensions.is_empty()
        {
            let (tx, rx) = mpsc::channel(NOTIFICATION_QUEUE);
            let task = tokio::spawn(deliver_notifications(rx, handler.clone()));
            (Some(tx), Some(task))
        } else {
            (None, None)
        };
        Self {
            notes,
            notifier,
            requests: JoinSet::new(),
            inflight: HashMap::new(),
            http,
        }
    }
}

/// Hand each notification to the handlers, one at a time, in order.
async fn deliver_notifications(
    mut notes: mpsc::Receiver<(JsonRpcNotification, Option<String>)>,
    handler: ClientHandlers,
) {
    while let Some((n, completed_elicitation)) = notes.recv().await {
        // `elicitation/complete` reaches the elicitation handler's dedicated
        // hook; everything reaches the notification observer.
        if let (Some(h), Some(id)) = (&handler.elicitation, completed_elicitation) {
            h.on_elicitation_complete(id).await;
        }
        // An extension's own notifications are its alone.
        if let Some(extension) = handler.claiming_notification(&n.method) {
            extension.on_notification(&n.method, n.params).await;
        } else if let Some(h) = &handler.notifications {
            h.on_notification(n.method, n.params).await;
        }
    }
}

/// Route one inbound frame. Returns `Some(reply)` for an *inline* reply the
/// actor must write (`ping`, and the no-handler `-32601` case); handled
/// server→client requests are dispatched on a spawned task that replies via
/// `weak_out`.
fn route_inbound(
    msg: JsonRpcMessage,
    failure: Option<turbomcp_service::TransportFailure>,
    related: Option<&RequestId>,
    state: &SessionState,
    dispatch: &mut Dispatch,
) -> Option<JsonRpcMessage> {
    let SessionState {
        pending,
        handler,
        weak_out,
        cache,
        negotiated,
        progress,
        subscriptions,
        inbound: _,
        input,
    } = state;
    match msg {
        JsonRpcMessage::Response(resp) => {
            // A call with its own handlers that became a `2025-11-25` task:
            // the task's requests are the call's from this frame on.
            if !input.is_empty()
                && let Some(id) = &resp.id
                && let Some(task_id) = resp
                    .result
                    .as_ref()
                    .filter(|r| r.get("content").is_none())
                    .and_then(|r| r.get("task"))
                    .and_then(|t| t.get("taskId"))
                    .and_then(Value::as_str)
            {
                input.became_task(id, task_id);
            }
            complete_pending(resp, pending, subscriptions, failure);
            None
        }
        // Progress for a call that asked for it goes to that call, in order.
        JsonRpcMessage::Notification(n)
            if n.method == notification::PROGRESS && progress.deliver(n.params.as_ref()) =>
        {
            None
        }
        JsonRpcMessage::Notification(n) => {
            // `subscriptions/listen` is answered by its acknowledgement, not by
            // a JSON-RPC response (only failures answer in band), so the ack is
            // what completes the waiting request. It still reaches the handler
            // below like any other notification.
            if n.method == notification::SUBSCRIPTIONS_ACKNOWLEDGED
                && let Some(id) = acknowledged_subscription_id(n.params.as_ref())
            {
                complete_pending_ok(&id, n.params.clone().unwrap_or(Value::Null), pending);
            }
            // Invalidate cached responses the notification obsoletes, then
            // hand it to the user's handler (default: ignore).
            if let Some(cache) = cache {
                cache.on_notification(&n.method, n.params.as_ref());
            }
            // "Receivers of cancellation notifications SHOULD: Stop processing
            // the cancelled request; Free associated resources; Not send a
            // response for the cancelled request." The server gave up on one
            // of its own requests to us: stop the handler, and its reply with it.
            if n.method == notification::CANCELLED
                && let Some(id) = n
                    .params
                    .as_ref()
                    .and_then(|p| p.get("requestId"))
                    .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok())
                && let Some(task) = dispatch.inflight.remove(&id)
            {
                task.abort();
            }
            // "Clients MUST ignore completion notifications for unknown or
            // already-completed elicitation IDs." Claiming the id here, once,
            // is what makes both halves of that true: an id this client was
            // never sent is not in the set, and a second notification for the
            // same id no longer is.
            let recognized = (n.method == notification::ELICITATION_COMPLETE)
                .then(|| {
                    n.params
                        .as_ref()
                        .and_then(|p| p.get("elicitationId"))
                        .and_then(Value::as_str)
                        .filter(|id| handler.claim_elicitation(id))
                        .map(ToOwned::to_owned)
                })
                .flatten();
            // A subscription's notifications go to its handle, and on to the
            // notification observer below like everything else.
            subscriptions.deliver(&n);
            match &dispatch.notes {
                None => {
                    tracing::trace!(method = %n.method, "client received notification (no handler)");
                }
                Some(notes) => {
                    if let Err(mpsc::error::TrySendError::Full(_)) = notes.try_send((n, recognized))
                    {
                        tracing::warn!(
                            "notification handler is {NOTIFICATION_QUEUE} behind; dropped a notification"
                        );
                    }
                }
            }
            None
        }
        // Liveness is a protocol obligation rather than an application one:
        // "the receiver MUST respond promptly with an empty response" (ping
        // spec), so it is answered here, inline, whether or not this client
        // installed a handler — and without queueing behind one that is busy
        // asking a human something, which would defeat the point of a ping.
        // `2026-07-28` dropped `ping`; answering a server that sends it anyway
        // costs nothing and beats claiming the method does not exist.
        JsonRpcMessage::Request(req)
            if dispatch.http
                && *negotiated
                    .lock()
                    .expect("negotiated version mutex poisoned")
                    == ProtocolVersion::V2026_07_28 =>
        {
            // Only a non-compliant server sends a request here, and there is
            // no way to answer it that the spec allows.
            tracing::debug!(method = %req.method, "server request on a wire with no replies; ignored");
            None
        }
        JsonRpcMessage::Request(req) if req.method == request::PING => {
            Some(JsonRpcResponse::success(req.id, serde_json::json!({})).into())
        }
        JsonRpcMessage::Request(req) if dispatch.inflight.len() >= MAX_INBOUND_REQUESTS => {
            tracing::warn!(method = %req.method, "too many server requests in flight; refusing one");
            Some(JsonRpcMessage::Response(JsonRpcResponse::error(
                req.id,
                JsonRpcError {
                    code: turbomcp_core::codes::INTERNAL_ERROR,
                    message: "client is busy: too many server requests in flight".to_owned(),
                    data: None,
                },
            )))
        }
        JsonRpcMessage::Request(req) => {
            // SEP-2260: a request that belongs to a call made with its own
            // handlers goes to them (see `crate::input`).
            let in_flight = pending.lock().expect("pending mutex poisoned").len();
            // The task a `2025-11-25` task's request names says whose it is
            // where no stream does (stdio, WebSocket).
            let task = req
                .params
                .as_ref()
                .and_then(|p| p.get("_meta"))
                .and_then(|m| m.get(turbomcp_core::meta::keys::RELATED_TASK))
                .and_then(|t| t.get("taskId"))
                .and_then(serde_json::Value::as_str)
                .map(|id| RouteKey::Task(id.to_owned()));
            let keys: Vec<RouteKey> = related
                .cloned()
                .map(RouteKey::Request)
                .into_iter()
                .chain(task)
                .collect();
            let handler = match input.answerer(&keys, handler, in_flight) {
                Answerer::Handlers(handlers) => handlers,
                Answerer::Ambiguous if handler.answers(&req.method) => handler.clone(),
                Answerer::Ambiguous => {
                    tracing::warn!(
                        method = %req.method,
                        "a server request with several calls in flight could belong to any; refused"
                    );
                    return Some(JsonRpcMessage::Response(JsonRpcResponse::error(
                        req.id,
                        JsonRpcError {
                            code: turbomcp_core::codes::INTERNAL_ERROR,
                            message: format!(
                                "cannot tell which of several calls in flight this {} belongs to",
                                req.method
                            ),
                            data: None,
                        },
                    )));
                }
            };
            match handler.is_empty() {
                // Dispatch on a task so a slow handler (user interaction) doesn't
                // head-of-line-block inbound reads; reply via the WeakSender.
                // `dispatch_server_request` answers `-32601` for a method whose own
                // handler is unregistered, so an unrelated one being present cannot
                // make this client look more capable than it declared.
                false => {
                    // Claim the id before the dispatch task starts: a server that
                    // answers its own URL-mode elicitation immediately would
                    // otherwise race the registration and lose the completion.
                    let version = negotiated
                        .lock()
                        .expect("negotiated version mutex poisoned")
                        .clone();
                    handler.expect_elicitation(&version, &req.method, req.params.as_ref());
                    let handlers = handler.clone();
                    let weak_out = weak_out.clone();
                    let request_id = req.id.clone();
                    // A request a task made answers with the task named too: "All
                    // requests, notifications, and responses related to a task
                    // MUST include the `io.modelcontextprotocol/related-task` key".
                    let related_task = req
                        .params
                        .as_ref()
                        .and_then(|p| p.get("_meta"))
                        .and_then(|m| m.get(turbomcp_core::meta::keys::RELATED_TASK))
                        .cloned();
                    let task = dispatch.requests.spawn(async move {
                        let id = req.id.clone();
                        // A user handler that panics used to take the reply down
                        // with it: the task unwound before the send below, and the
                        // server sat out its own 120s timeout with no way to tell a
                        // buggy client from a slow human. Every request gets an
                        // answer, even a `-32603` one.
                        let outcome = turbomcp_service::catch_panic(dispatch_server_request(
                            &handlers,
                            &version,
                            &req.method,
                            req.params,
                        ))
                        .await
                        .unwrap_or_else(|detail| {
                            tracing::error!(
                                panic = detail,
                                method = %req.method,
                                "client handler panicked; answering -32603"
                            );
                            Err(JsonRpcError {
                                code: turbomcp_core::codes::INTERNAL_ERROR,
                                message: "client handler panicked".to_owned(),
                                data: None,
                            })
                        });
                        let reply = match outcome {
                            Ok(mut value) => {
                                if let (Some(task), Some(result)) =
                                    (related_task, value.as_object_mut())
                                    && let Some(meta) = result
                                        .entry("_meta")
                                        .or_insert_with(|| serde_json::json!({}))
                                        .as_object_mut()
                                {
                                    meta.insert(
                                        turbomcp_core::meta::keys::RELATED_TASK.to_owned(),
                                        task,
                                    );
                                }
                                JsonRpcResponse::success(id.clone(), value)
                            }
                            Err(err) => JsonRpcResponse::error(id.clone(), err),
                        };
                        if let Some(tx) = weak_out.upgrade() {
                            let _ = tx
                                .send((JsonRpcMessage::Response(reply), Extensions::new()))
                                .await;
                        }
                        id
                    });
                    dispatch.inflight.insert(request_id, task);
                    None
                }
                // No handler configured: refuse politely rather than hang the server.
                true => {
                    tracing::debug!(method = %req.method, "server→client request with no handler");
                    Some(JsonRpcMessage::Response(JsonRpcResponse::error(
                        req.id,
                        JsonRpcError {
                            code: turbomcp_core::codes::METHOD_NOT_FOUND,
                            message: format!("method not found: {}", req.method),
                            data: None,
                        },
                    )))
                }
            }
        }
    }
}

/// The listen request id an acknowledgement names, from its `_meta`.
///
/// The subscriptions spec pins this to the listen request's JSON-RPC id
/// **verbatim** — a number stays a number — so it round-trips through
/// `RequestId`'s untagged representation.
fn acknowledged_subscription_id(params: Option<&Value>) -> Option<RequestId> {
    let raw = params?.get("_meta")?.get(meta::keys::SUBSCRIPTION_ID)?;
    serde_json::from_value(raw.clone()).ok()
}

/// Complete a waiting request with a successful value, if one is waiting.
fn complete_pending_ok(id: &RequestId, value: Value, pending: &Arc<Pending>) {
    let waiter = pending.lock().expect("pending mutex poisoned").remove(id);
    if let Some(waiter) = waiter {
        let _ = waiter.send(Ok(value));
    }
}

/// What a client does with one bad frame from the server, which never costs
/// it the connection: a stdio server that prints a banner, or answers with a
/// shape this SDK can't decode, is still a server worth talking to.
///
/// - A broken *response* fails the request waiting on its id right away,
///   rather than leaving the caller to wait out its timeout.
/// - A broken *request* the server can be told about (its id was readable)
///   gets its Invalid Request answer, which is the reply returned here.
/// - Anything else (a banner line, an unreadable id) is skipped. Answering
///   stray stdout noise with error frames would only give the server
///   something to choke on.
fn invalid_from_server(bad: &InvalidFrame, pending: &Arc<Pending>) -> Option<JsonRpcMessage> {
    tracing::warn!(error = %bad, "invalid frame from server; skipping it");
    let id = bad.id.as_ref()?;
    if bad.is_response {
        let waiter = pending.lock().expect("pending mutex poisoned").remove(id);
        if let Some(waiter) = waiter {
            let _ = waiter.send(Err(ClientError::Decode(format!(
                "the server's response was not a valid JSON-RPC message: {bad}"
            ))));
        }
        return None;
    }
    bad.response().map(Into::into)
}

/// Deliver a response to the request waiting on its id, if any.
fn complete_pending(
    resp: JsonRpcResponse,
    pending: &Arc<Pending>,
    subscriptions: &SubscriptionRoutes,
    failure: Option<turbomcp_service::TransportFailure>,
) {
    let Some(id) = &resp.id else {
        // The server couldn't read a frame of ours well enough to find its
        // id. Nothing is waiting on "no id", so all that's left is to say so.
        tracing::warn!(error = ?resp.error, "server answered an unreadable frame");
        return;
    };
    let waiter = pending.lock().expect("pending mutex poisoned").remove(id);
    let Some(waiter) = waiter else {
        // A listen request completes at its acknowledgement; a response to it
        // afterwards is the server closing the subscription gracefully.
        if !subscriptions.close(id) {
            tracing::debug!(id = ?resp.id, "response for unknown/duplicate request id (dropped)");
        }
        return;
    };
    let outcome = match (failure, resp.error) {
        (Some(turbomcp_service::TransportFailure::StreamLost), _) => Err(ClientError::StreamLost),
        (Some(turbomcp_service::TransportFailure::Http(err)), _) => {
            Err(ClientError::Http(Box::new(err)))
        }
        (Some(other), _) => Err(ClientError::Protocol(other.to_string())),
        (None, Some(err)) => Err(ClientError::Rpc(err)),
        (None, None) => Ok(resp.result.unwrap_or(Value::Null)),
    };
    // The caller may have timed out and dropped the receiver — that's fine.
    let _ = waiter.send(outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The two carve-outs are MUST NOTs, and neither is reachable from an
    /// integration test: the handshake completes before a caller holds a client
    /// to abandon it with, and a task-augmented call is answered promptly with
    /// a handle rather than left in flight.
    #[test]
    fn the_handshake_is_never_cancelled() {
        assert!(!cancellable(request::INITIALIZE, None));
        assert!(!cancellable(request::DISCOVER, None));
    }

    #[test]
    fn a_task_augmented_request_defers_to_tasks_cancel() {
        let augmented = json!({ "name": "slow", "task": { "ttl": 1000 } });
        assert!(!cancellable("tools/call", Some(&augmented)));
    }

    #[test]
    fn an_ordinary_request_is_cancellable() {
        assert!(cancellable("tools/list", None));
        // A draft task is server-initiated, so a `tools/call` with no `task`
        // field is exactly the case where the notification is all we have.
        assert!(cancellable("tools/call", Some(&json!({ "name": "slow" }))));
        // Including the task methods themselves.
        assert!(cancellable("tasks/get", Some(&json!({ "taskId": "t1" }))));
    }
}
