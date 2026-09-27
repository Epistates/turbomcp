//! The transport ↔ service driver loop.
//!
//! [`serve`] reads frames from a [`Transport`], runs each through an
//! [`McpService`] (the dispatcher, wrapped in whatever middleware), and writes
//! back any response. Notifications (service returns `None`) produce no write.
//!
//! ## Concurrency model (the single-writer-actor)
//!
//! A slow handler must not head-of-line-block the reader, and outbound frames
//! must never interleave on the wire. The driver therefore separates the two
//! halves:
//!
//! - **Reader / dispatch:** the reader calls a *cloned* service for each
//!   inbound frame, in arrival order, and spawns the returned future, so N
//!   requests are in flight concurrently while the service still sees them in
//!   the order they came.
//! - **Writer actor:** every task funnels its response through a single
//!   `mpsc` channel, and one arm of the [`tokio::select!`] loop is the sole
//!   writer to the transport — frames are serialized, never interleaved. Each
//!   request carries a [`Peer`] onto that channel, so server-initiated
//!   messages (notifications, inline requests) share the same ordered writer.
//!
//! **Admission** uses separate bounded application and control budgets. Excess
//! application requests are rejected; responses and cancellation can still
//! progress when every application slot is occupied.
//!
//! **Graceful shutdown** (PLAN §4.13): firing the configured
//! [`CancellationToken`] stops the reader, then in-flight handlers are given
//! `drain_timeout` to finish and flush their replies before the transport is
//! closed; stragglers past the deadline are aborted.
//!
//! ## What the driver attaches
//!
//! Every request goes to the service as an [`McpRequest`] carrying this
//! connection's [`ConnectionId`] (one per `serve` call), its [`Peer`], and the
//! connection's authenticated [`Identity`] when the transport established one.
//! They travel beside the message, not in it, so nothing a client sends can
//! assert them.

use std::future::poll_fn;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::FutureExt as _;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio::time::Instant;
use turbomcp_core::CancellationToken;
use turbomcp_core::{ConnectionId, Identity, JsonRpcMessage, McpRequest};

use crate::{McpService, Peer, ProtocolError, Transport, catch_handler_panic};

/// Tuning for the [`serve_with`] driver.
#[derive(Clone, Debug)]
pub struct ServeConfig {
    /// Maximum concurrently in-flight application handlers; excess requests fail.
    /// Doubles as the outbound channel capacity. Default: 1024.
    pub max_in_flight: usize,
    /// On shutdown, how long in-flight handlers have to finish and flush before
    /// they are aborted. Default: 30s.
    pub drain_timeout: Duration,
    /// How long one outbound frame may wait on the transport before the peer
    /// is treated as gone (it stopped reading). Default: 30s.
    pub write_timeout: Duration,
    /// Fire to begin graceful shutdown. Default: a token that is never fired
    /// (the driver runs until the peer closes the stream).
    pub shutdown: CancellationToken,
    /// The connection's authenticated principal, attached to every request.
    /// Set by transports that authenticate at connection time (e.g. a
    /// WebSocket bearer check at the upgrade); `None` leaves requests
    /// anonymous. Default: `None`.
    pub identity: Option<Identity>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 1024,
            drain_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            shutdown: CancellationToken::new(),
            identity: None,
        }
    }
}

/// Drive `service` from `transport` until the peer closes the stream, with the
/// default [`ServeConfig`].
///
/// Returns `Ok(())` on a clean end-of-stream. A transport failure becomes
/// [`ProtocolError::Transport`]. Neither a service error nor a malformed frame
/// is fatal: each is answered with a JSON-RPC error and reading continues, so
/// one bad request can't tear down the connection.
///
/// # Errors
/// Propagates transport I/O failures and the service's readiness error, if any.
pub async fn serve<T, S>(transport: T, service: S) -> Result<(), ProtocolError>
where
    T: Transport,
    S: McpService + Clone,
    S::Future: Send + 'static,
{
    serve_with(transport, service, ServeConfig::default()).await
}

/// Drive `service` from `transport` with explicit [`ServeConfig`].
///
/// # Errors
/// Propagates transport I/O failures and the service's readiness error, if any.
pub async fn serve_with<T, S>(
    mut transport: T,
    service: S,
    config: ServeConfig,
) -> Result<(), ProtocolError>
where
    T: Transport,
    S: McpService + Clone,
    S::Future: Send + 'static,
{
    let ServeConfig {
        max_in_flight,
        drain_timeout,
        write_timeout,
        shutdown,
        identity,
    } = config;
    let capacity = max_in_flight.max(1);

    // In-process connection identity: lets the dispatcher scope in-flight
    // request cancellation to this connection. Needs only process-uniqueness
    // (it never leaves the process), so a counter beats a uuid.
    static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);
    let connection_id = ConnectionId::new(format!(
        "conn-{}",
        NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)
    ));

    let (tx, mut rx) = mpsc::channel::<JsonRpcMessage>(capacity);
    // Server-initiated messages (subscription pushes, inline requests) ride the
    // same single-writer actor through this handle. It holds the queue weakly:
    // `tx` below is what keeps it open, and dropping `tx` is what closes it.
    let peer = Peer::new(connection_id.clone(), &tx);
    let limiter = Arc::new(Semaphore::new(capacity));
    let controls = Arc::new(Semaphore::new(64));
    let mut handlers: JoinSet<()> = JoinSet::new();
    // `svc` is always the instance most recently driven to readiness; we call a
    // clone of it and keep a fresh clone for the next frame (the canonical tower
    // "drive-to-ready, clone, call" concurrency pattern).
    let svc = service;

    let result = loop {
        // Unbiased on purpose. Polling outbound first let a handler that
        // emits notifications faster than the peer reads keep this branch
        // winning, so the reader never ran and the peer's
        // `notifications/cancelled` for that very handler sat unread.
        tokio::select! {
            // Writes stay ordered: this is the only branch that writes.
            //
            // Once a frame is out of `rx` it has to be written: nothing else
            // holds a copy. Racing the shutdown token against the write here
            // dropped it instead, and — since an abandoned write may have
            // emitted a partial frame — also marked the stream unusable, which
            // skips the drain and aborts every other in-flight handler. The
            // write is bounded by `write_timeout`, so shutdown loses little by
            // letting it finish.
            Some(out) = rx.recv() => {
                match tokio::time::timeout(write_timeout, transport.send(out)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => break Err(ProtocolError::Transport(e.to_string())),
                    Err(_) => break Err(ProtocolError::Transport("write deadline exceeded".into())),
                }
            }
            // Reap finished handlers so the JoinSet can't grow unbounded.
            Some(_joined) = handlers.join_next(), if !handlers.is_empty() => {}
            // Begin graceful shutdown.
            () = shutdown.cancelled() => break Ok(()),
            // Read the next inbound frame.
            frame = transport.recv() => {
                match frame {
                    // One bad frame is the peer's bug, not a dead stream:
                    // answer it (unless it was a response) and read on.
                    // Tearing down here used to abort every request in flight
                    // on the connection over one malformed line.
                    Err(e) => match T::invalid_frame(e) {
                        Ok(bad) => {
                            tracing::warn!(error = %bad, "invalid frame from peer");
                            if let Some(reply) = bad.response()
                                && tx.try_send(reply.into()).is_err()
                            {
                                break Err(ProtocolError::Transport("outbound capacity exceeded".into()));
                            }
                        }
                        Err(e) => break Err(ProtocolError::Transport(e.to_string())),
                    },
                    Ok(None) => break Ok(()), // clean EOF
                    Ok(Some(msg)) => {
                        let application = matches!(msg, JsonRpcMessage::Request(_));
                        let budget = if application { &limiter } else { &controls };
                        let permit = match Arc::clone(budget).try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                if let JsonRpcMessage::Request(req) = msg {
                                    let reply = turbomcp_core::JsonRpcResponse::error(req.id,
                                        turbomcp_core::JsonRpcError { code: -32000,
                                            message: "server at capacity".into(), data: None }).into();
                                    // Never park the reader behind its own writer queue.
                                    if tx.try_send(reply).is_err() {
                                        break Err(ProtocolError::Transport("outbound capacity exceeded".into()));
                                    }
                                    continue;
                                }
                                break Err(ProtocolError::Transport("control capacity exceeded".into()));
                            }
                        };
                        let mut ready = svc.clone();
                        let out_tx = tx.clone();
                        // Kept for the error and panic paths: a request whose
                        // handler fails or unwinds is still owed a response.
                        let reply_id = match &msg {
                            JsonRpcMessage::Request(r) => Some(r.id.clone()),
                            _ => None,
                        };
                        // `call` runs here, on the reader, whenever the service
                        // is ready without waiting (the dispatcher always is):
                        // that keeps the service seeing frames in arrival
                        // order, so a cancellation can't overtake the request
                        // it cancels. A service that isn't ready yet is driven
                        // to readiness on the handler task instead, never on
                        // the reader, which also has to keep writing.
                        let mut request = McpRequest::new(msg)
                            .with(connection_id.clone())
                            .with(peer.clone());
                        if let Some(identity) = &identity {
                            request.extensions.insert(identity.clone());
                        }
                        let call: futures::future::BoxFuture<'static, _> =
                            match poll_fn(|cx| ready.poll_ready(cx)).now_or_never() {
                                Some(Ok(())) => {
                                    let fut = ready.call(request);
                                    Box::pin(fut)
                                }
                                Some(Err(e)) => Box::pin(async move { Err(e) }),
                                None => Box::pin(async move {
                                    poll_fn(|cx| ready.poll_ready(cx)).await?;
                                    ready.call(request).await
                                }),
                            };
                        handlers.spawn(async move {
                            let _permit = permit; // released when the handler ends
                            match catch_handler_panic(reply_id.clone(), call).await {
                                Ok(Some(reply)) => {
                                    let _ = out_tx.send(reply).await;
                                }
                                Ok(None) => {} // notification: no reply
                                // "The Server MUST reply with a Response,
                                // except for in the case of Notifications":
                                // a service error is still an answer. It used
                                // to be logged and dropped, leaving the peer
                                // to wait out its timeout.
                                Err(e) => {
                                    tracing::warn!(error = %e, "rpc handler failed");
                                    if let Some(id) = reply_id {
                                        let _ = out_tx.send(e.into_response(id).into()).await;
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
    };

    // A failed or timed-out framed write may have emitted a partial frame.
    // Never append another frame to that stream during graceful drain.
    if result.is_err() {
        handlers.abort_all();
        return result;
    }

    // Drain, phase 1: in-flight handlers may still emit server-initiated
    // messages (progress, inline bidi requests) through their `Peer`, so the
    // queue stays open until they finish. Keep writing
    // replies and reaping handlers until the set is empty or the deadline
    // forces an abort.
    let deadline = Instant::now() + drain_timeout;
    loop {
        if handlers.is_empty() {
            break;
        }
        tokio::select! {
            biased;
            Some(out) = rx.recv() => {
                if !matches!(tokio::time::timeout_at(deadline, transport.send(out)).await, Ok(Ok(()))) {
                    handlers.abort_all();
                    break;
                }
            }
            Some(_joined) = handlers.join_next() => {}
            () = tokio::time::sleep_until(deadline) => {
                handlers.abort_all();
                break;
            }
        }
    }

    // Drain, phase 2: drop our own sender, the only strong one (every `Peer`
    // is weak), so `rx` reports closure once the queue empties; flush what's
    // left.
    drop(peer);
    drop(tx);
    let close = tokio::time::timeout_at(deadline, async {
        while let Some(out) = rx.recv().await {
            transport
                .send(out)
                .await
                .map_err(|e| ProtocolError::Transport(e.to_string()))?;
        }
        transport
            .graceful_shutdown(deadline.into_std())
            .await
            .map_err(|e| ProtocolError::Transport(e.to_string()))
    })
    .await
    .unwrap_or(Ok(()));
    match result {
        Err(e) => Err(e),
        Ok(()) => close,
    }
}
