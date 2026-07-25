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
//! - **Reader / dispatch:** each inbound frame is handed to a *cloned* service
//!   on its own spawned task, so N requests are in flight concurrently.
//! - **Writer actor:** every task funnels its response through a single
//!   `mpsc` channel, and one arm of the [`tokio::select!`] loop is the sole
//!   writer to the transport — frames are serialized, never interleaved. The
//!   `mpsc::Sender` is the seam Phase 6 clones into the subscription registry so
//!   server-initiated notifications share the same ordered writer.
//!
//! **Backpressure** is a [`Semaphore`] sized to `max_in_flight`: the reader
//! parks on `acquire` once that many handlers are outstanding, which (on stdio's
//! single process) naturally serializes load instead of unbounded-spawning.
//!
//! **Graceful shutdown** (PLAN §4.13): firing the configured
//! [`CancellationToken`] stops the reader, then in-flight handlers are given
//! `drain_timeout` to finish and flush their replies before the transport is
//! closed; stragglers past the deadline are aborted.
//!
//! ## The driver is the trust boundary
//!
//! The driver is the first code to see a frame off the wire, so it owns the
//! internal-`_meta` hygiene ([`meta::sanitize_inbound`]): forged
//! `io.turbomcp.internal/*` keys are stripped, then the driver asserts its own
//! [`meta::internal::CONNECTION_ID`] (one id per `serve` call). Layers below
//! (session adapter, dispatcher) trust internal keys — they must always sit
//! under a sanitizing boundary like this driver or the HTTP endpoint.

use std::future::poll_fn;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use turbomcp_core::{JsonRpcMessage, meta};

use crate::{McpService, ProtocolError, Transport, catch_handler_panic};

/// Tuning for the [`serve_with`] driver.
#[derive(Clone, Debug)]
pub struct ServeConfig {
    /// Maximum concurrently in-flight handlers; the reader parks once reached.
    /// Doubles as the outbound channel capacity. Default: 1024.
    pub max_in_flight: usize,
    /// On shutdown, how long in-flight handlers have to finish and flush before
    /// they are aborted. Default: 30s.
    pub drain_timeout: Duration,
    /// Fire to begin graceful shutdown. Default: a token that is never fired
    /// (the driver runs until the peer closes the stream).
    pub shutdown: CancellationToken,
    /// The connection's authenticated principal, stamped into every inbound
    /// message as [`meta::internal::IDENTITY`] after sanitization (the same
    /// shape the HTTP transport injects: `{ "sub": String, "claims": Object }`).
    /// Set by transports that authenticate at connection time (e.g. a
    /// WebSocket bearer check at the upgrade); `None` leaves requests
    /// anonymous. Default: `None`.
    pub identity: Option<serde_json::Value>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 1024,
            drain_timeout: Duration::from_secs(30),
            shutdown: CancellationToken::new(),
            identity: None,
        }
    }
}

/// Drive `service` from `transport` until the peer closes the stream, with the
/// default [`ServeConfig`].
///
/// Returns `Ok(())` on a clean end-of-stream. A transport failure becomes
/// [`ProtocolError::Transport`]. Per-request handler errors are logged, not
/// fatal — one bad request must not tear down the connection.
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
        shutdown,
        identity,
    } = config;
    let capacity = max_in_flight.max(1);

    // In-process connection identity: lets the dispatcher scope in-flight
    // request cancellation to this connection. Needs only process-uniqueness
    // (it never leaves the process), so a counter beats a uuid.
    static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);
    let connection_id = format!("conn-{}", NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed));

    let (tx, mut rx) = mpsc::channel::<JsonRpcMessage>(capacity);
    // Publish this connection's ordered writer so server-initiated messages
    // (subscription pushes, bidi requests) ride the same single-writer actor.
    let writer_registration = crate::outbound::register(&connection_id, tx.clone());
    let limiter = Arc::new(Semaphore::new(capacity));
    let mut handlers: JoinSet<()> = JoinSet::new();
    // `svc` is always the instance most recently driven to readiness; we call a
    // clone of it and keep a fresh clone for the next frame (the canonical tower
    // "drive-to-ready, clone, call" concurrency pattern).
    let mut svc = service;

    let result = loop {
        tokio::select! {
            biased;
            // 1. Flush outbound first so replies stay prompt and writes ordered.
            Some(out) = rx.recv() => {
                if let Err(e) = transport.send(out).await {
                    break Err(ProtocolError::Transport(e.to_string()));
                }
            }
            // 2. Reap finished handlers so the JoinSet can't grow unbounded.
            Some(_joined) = handlers.join_next(), if !handlers.is_empty() => {}
            // 3. Begin graceful shutdown.
            () = shutdown.cancelled() => break Ok(()),
            // 4. Read the next inbound frame.
            frame = transport.recv() => {
                match frame {
                    Err(e) => break Err(ProtocolError::Transport(e.to_string())),
                    Ok(None) => break Ok(()), // clean EOF
                    Ok(Some(mut msg)) => {
                        // Trust boundary: strip forged internal keys, then
                        // assert this connection's identity.
                        meta::sanitize_inbound(&mut msg);
                        meta::set_request_meta(
                            &mut msg,
                            meta::internal::CONNECTION_ID,
                            connection_id.clone().into(),
                        );
                        if let Some(principal) = &identity {
                            meta::set_request_meta(
                                &mut msg,
                                meta::internal::IDENTITY,
                                principal.clone(),
                            );
                        }
                        // Backpressure: park the reader until a slot frees.
                        let Ok(permit) = Arc::clone(&limiter).acquire_owned().await else {
                            break Ok(()); // semaphore closed — shouldn't happen
                        };
                        // Drive readiness on `svc`, then call a clone of *that*
                        // instance and retain a fresh clone for the next frame.
                        if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
                            break Err(e);
                        }
                        let mut ready = svc.clone();
                        std::mem::swap(&mut ready, &mut svc);
                        let out_tx = tx.clone();
                        // Kept for the panic path: a handler that unwinds still
                        // owes this request a response (see `catch_handler_panic`).
                        let reply_id = match &msg {
                            JsonRpcMessage::Request(r) => Some(r.id.clone()),
                            _ => None,
                        };
                        handlers.spawn(async move {
                            let _permit = permit; // released when the handler ends
                            match catch_handler_panic(reply_id, ready.call(msg)).await {
                                Ok(Some(reply)) => {
                                    let _ = out_tx.send(reply).await;
                                }
                                Ok(None) => {} // notification: no reply
                                Err(e) => tracing::warn!(error = %e, "rpc handler failed"),
                            }
                        });
                    }
                }
            }
        }
    };

    // Drain, phase 1: in-flight handlers may still emit server-initiated
    // messages (progress, inline bidi requests) through the outbound table, so
    // the writer registration stays live until they finish. Keep writing
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
                if transport.send(out).await.is_err() {
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

    // Drain, phase 2: unregister the outbound writer (its sender clone would
    // hold the channel open forever) and drop our own sender, so `rx` reports
    // closure once the last straggler clone drops; flush what's left.
    drop(writer_registration);
    drop(tx);
    while let Some(out) = rx.recv().await {
        if transport.send(out).await.is_err() {
            break;
        }
    }

    // Close the transport regardless of drain outcome; prefer reporting the
    // driver loop's error over the close error.
    let close = transport
        .graceful_shutdown(deadline.into_std())
        .await
        .map_err(|e| ProtocolError::Transport(e.to_string()));
    match result {
        Err(e) => Err(e),
        Ok(()) => close,
    }
}
