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
//!   the [`ClientHandler`]; a server→client `Request` is dispatched to that same
//!   handler (elicit/sample/roots) on a spawned task whose reply is sent back
//!   through a [`WeakSender`](mpsc::WeakSender) — or, with no handler, answered
//!   `-32601` inline.
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
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use turbomcp_core::{
    JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestId, meta,
};
use turbomcp_protocol::methods::{notification, request};
use turbomcp_service::Transport;

use crate::cache::ResponseCache;
use crate::error::{ClientError, ClientResult};
use crate::handler::{ClientHandler, dispatch_server_request};

/// Default per-request timeout — a request with no answer in this window fails
/// with [`ClientError::Timeout`] rather than hanging forever.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The waiting side of in-flight requests: id → the oneshot its caller awaits.
type Pending = Mutex<HashMap<RequestId, oneshot::Sender<Result<Value, JsonRpcError>>>>;

/// Shared connection state, held by every [`Connection`] clone.
struct Inner {
    /// Frames the client wants to send (the actor owns the receiver). The actor
    /// holds only a `WeakSender`, so dropping all handles closes the channel.
    outbound: mpsc::Sender<JsonRpcMessage>,
    /// In-flight requests awaiting a response (shared with the actor).
    pending: Arc<Pending>,
    /// Monotonic request-id source (process-local; integer ids).
    next_id: AtomicI64,
    /// How long [`Connection::request`] waits before giving up.
    request_timeout: Duration,
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
        Self::connect(transport, DEFAULT_REQUEST_TIMEOUT, None)
    }

    /// Spawn the connection actor with an explicit per-request timeout and no
    /// client-serving handler.
    pub fn with_timeout<T>(transport: T, request_timeout: Duration) -> Self
    where
        T: Transport,
    {
        Self::connect(transport, request_timeout, None)
    }

    /// Spawn the connection actor with a timeout and an optional
    /// [`ClientHandler`] for server→client requests (elicit/sample/roots).
    pub fn connect<T>(
        transport: T,
        request_timeout: Duration,
        handler: Option<Arc<dyn ClientHandler>>,
    ) -> Self
    where
        T: Transport,
    {
        Self::connect_with_cache(transport, request_timeout, handler, None)
    }

    /// [`connect`](Self::connect), plus a [`ResponseCache`] the actor
    /// invalidates on inbound `*_list_changed` / `resources/updated`
    /// notifications (the [`Client`](crate::Client) wires this).
    pub(crate) fn connect_with_cache<T>(
        transport: T,
        request_timeout: Duration,
        handler: Option<Arc<dyn ClientHandler>>,
        cache: Option<Arc<ResponseCache>>,
    ) -> Self
    where
        T: Transport,
    {
        // Capacity mirrors the server driver's default outbound buffer.
        let (tx, rx) = mpsc::channel::<JsonRpcMessage>(1024);
        let pending: Arc<Pending> = Arc::new(Mutex::new(HashMap::new()));
        let weak_out = tx.downgrade();
        tokio::spawn(actor(
            transport,
            rx,
            Arc::clone(&pending),
            weak_out,
            handler,
            cache,
        ));
        Self {
            inner: Arc::new(Inner {
                outbound: tx,
                pending,
                next_id: AtomicI64::new(1),
                request_timeout,
            }),
        }
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
        let id = RequestId::Number(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .expect("pending mutex poisoned")
            .insert(id.clone(), reply_tx);

        let method = method.into();
        let notify_on_abandon = cancellable(&method, params.as_ref());
        let msg = JsonRpcMessage::Request(JsonRpcRequest::new(id.clone(), method, params));
        if self.inner.outbound.send(msg).await.is_err() {
            self.forget(&id);
            return Err(ClientError::Closed);
        }

        // Armed from here until the request resolves. Both ways a caller can
        // stop waiting run through its `Drop`: the timeout arm below returns
        // while it is still armed, and a caller that drops this future outright
        // (a `select!` losing its race, its own deadline firing) never reaches
        // the disarm at all.
        let mut abandon = AbandonGuard {
            conn: self,
            id: id.clone(),
            reason: Some("the caller dropped the request"),
            notify: notify_on_abandon,
        };

        let outcome = match tokio::time::timeout(self.inner.request_timeout, reply_rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(err))) => Err(ClientError::Rpc(err)),
            // The actor dropped the sender (connection closed) before replying.
            Ok(Err(_recv)) => Err(ClientError::Closed),
            Err(_elapsed) => {
                abandon.reason = Some("the client's request timeout elapsed");
                return Err(ClientError::Timeout);
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
        use turbomcp_core::JsonRpcNotification;
        let msg = JsonRpcMessage::Notification(JsonRpcNotification::new(method, params));
        self.inner
            .outbound
            .send(msg)
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
            .send(msg)
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
    /// [`try_send`](mpsc::Sender::try_send). A full channel is the interesting
    /// case: dropping the frame there would silently restore the very bug this
    /// exists to fix, and "the client was busy" is exactly when a server is
    /// most worth telling. So a full channel hands the blocking send to a task
    /// instead — on the *same* channel, because a cancellation overtaking the
    /// request it names would reference something the server has never seen
    /// ("cancellation notifications MUST only reference requests that … are
    /// believed to still be in-progress").
    ///
    /// Two cases genuinely have nothing to do: a closed channel means the
    /// connection is gone and the server has stopped listening, and no runtime
    /// means this future was dropped somewhere that cannot spawn.
    fn cancel_on_wire(&self, id: &RequestId, reason: &str) {
        let params = serde_json::json!({ "requestId": id, "reason": reason });
        let msg = JsonRpcMessage::Notification(turbomcp_core::JsonRpcNotification::new(
            notification::CANCELLED,
            Some(params),
        ));
        match self.inner.outbound.try_send(msg) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(msg)) => {
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        let outbound = self.inner.outbound.clone();
                        handle.spawn(async move {
                            let _ = outbound.send(msg).await;
                        });
                    }
                    Err(_) => tracing::debug!(
                        request_id = ?id,
                        "dropped outside a runtime; notifications/cancelled not sent"
                    ),
                }
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

/// The connection actor: owns the transport, multiplexes both directions.
async fn actor<T>(
    mut transport: T,
    mut outbound: mpsc::Receiver<JsonRpcMessage>,
    pending: Arc<Pending>,
    weak_out: mpsc::WeakSender<JsonRpcMessage>,
    handler: Option<Arc<dyn ClientHandler>>,
    cache: Option<Arc<ResponseCache>>,
) where
    T: Transport,
{
    loop {
        tokio::select! {
            biased;
            // Outbound: a frame to put on the wire.
            out = outbound.recv() => {
                match out {
                    Some(msg) => {
                        if let Err(e) = transport.send(msg).await {
                            tracing::debug!(error = %e, "client transport send failed; closing");
                            break;
                        }
                    }
                    // All Connection handles dropped — nothing more to send.
                    None => break,
                }
            }
            // Inbound: the next frame from the server.
            frame = transport.recv() => {
                match frame {
                    Ok(Some(msg)) => {
                        if let Some(reply) = route_inbound(msg, &pending, &handler, &weak_out, &cache)
                            && let Err(e) = transport.send(reply).await {
                                tracing::debug!(error = %e, "client reply send failed; closing");
                                break;
                            }
                    }
                    Ok(None) => break, // clean EOF
                    Err(e) => {
                        tracing::debug!(error = %e, "client transport recv failed; closing");
                        break;
                    }
                }
            }
        }
    }

    // Connection is down: drop every waiting oneshot sender. A caller blocked in
    // `request` sees its receiver close and returns `ClientError::Closed`.
    // This happens *before* the close below, which can block on I/O — a caller
    // must learn the connection died promptly, not after a shutdown round trip.
    pending.lock().expect("pending mutex poisoned").clear();

    // Shut the transport down deliberately rather than by drop. Each transport
    // owes the peer something on the way out that dropping a socket does not
    // deliver: HTTP sends the spec's `DELETE` to terminate its session (absent
    // it, a long-lived server accumulates one session per client that ever
    // connected), and WebSocket sends its Close frame. Best-effort — the
    // connection is already going away, so a failure here has no one to tell.
    if let Err(e) = transport.close().await {
        tracing::debug!(error = %e, "client transport close failed");
    }
}

/// Route one inbound frame. Returns `Some(reply)` for an *inline* reply the
/// actor must write (`ping`, and the no-handler `-32601` case); handled
/// server→client requests are dispatched on a spawned task that replies via
/// `weak_out`.
fn route_inbound(
    msg: JsonRpcMessage,
    pending: &Arc<Pending>,
    handler: &Option<Arc<dyn ClientHandler>>,
    weak_out: &mpsc::WeakSender<JsonRpcMessage>,
    cache: &Option<Arc<ResponseCache>>,
) -> Option<JsonRpcMessage> {
    match msg {
        JsonRpcMessage::Response(resp) => {
            complete_pending(resp, pending);
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
            match handler {
                Some(handler) => {
                    let handler = Arc::clone(handler);
                    tokio::spawn(async move {
                        // `elicitation/complete` also reaches its dedicated
                        // hook; a malformed one (no string `elicitationId`)
                        // is an unknown id — ignored, per spec.
                        if n.method == notification::ELICITATION_COMPLETE
                            && let Some(id) = n
                                .params
                                .as_ref()
                                .and_then(|p| p.get("elicitationId"))
                                .and_then(Value::as_str)
                        {
                            handler.on_elicitation_complete(id.to_owned()).await;
                        }
                        handler.on_notification(n.method, n.params).await;
                    });
                }
                None => {
                    tracing::trace!(method = %n.method, "client received notification (no handler)");
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
        JsonRpcMessage::Request(req) if req.method == request::PING => {
            Some(JsonRpcResponse::success(req.id, serde_json::json!({})).into())
        }
        JsonRpcMessage::Request(req) => match handler {
            // Dispatch on a task so a slow handler (user interaction) doesn't
            // head-of-line-block inbound reads; reply via the WeakSender.
            Some(handler) => {
                let handler = Arc::clone(handler);
                let weak_out = weak_out.clone();
                tokio::spawn(async move {
                    let id = req.id.clone();
                    let reply =
                        match dispatch_server_request(handler.as_ref(), &req.method, req.params)
                            .await
                        {
                            Ok(value) => JsonRpcResponse::success(id, value),
                            Err(err) => JsonRpcResponse::error(id, err),
                        };
                    if let Some(tx) = weak_out.upgrade() {
                        let _ = tx.send(JsonRpcMessage::Response(reply)).await;
                    }
                });
                None
            }
            // No handler configured: refuse politely rather than hang the server.
            None => {
                tracing::debug!(method = %req.method, "server→client request with no handler");
                Some(JsonRpcMessage::Response(JsonRpcResponse::error(
                    req.id,
                    JsonRpcError {
                        code: -32601,
                        message: format!("method not found: {}", req.method),
                        data: None,
                    },
                )))
            }
        },
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

/// Deliver a response to the request waiting on its id, if any.
fn complete_pending(resp: JsonRpcResponse, pending: &Arc<Pending>) {
    let waiter = pending
        .lock()
        .expect("pending mutex poisoned")
        .remove(&resp.id);
    let Some(waiter) = waiter else {
        tracing::debug!(id = ?resp.id, "response for unknown/duplicate request id (dropped)");
        return;
    };
    let outcome = match resp.error {
        Some(err) => Err(err),
        None => Ok(resp.result.unwrap_or(Value::Null)),
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
