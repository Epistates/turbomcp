//! The [`Transport`] trait: a bidirectional [`JsonRpcMessage`] channel.
//!
//! Transports own framing (line-delimited for stdio, SSE-event-framed for HTTP)
//! and hand the codec complete frames. The serve loop drives a `Transport`
//! directly, so the trait uses return-position `impl Future` (native AFIT/RPITIT)
//! rather than boxed futures — no per-message allocation, fully monomorphized.
//!
//! # The production parity contract
//!
//! Every bundled server transport — stdio (`turbomcp-transport-stdio`),
//! WebSocket (`turbomcp-transport-ws`), and Streamable HTTP
//! (`turbomcp-transport-http`, a runner rather than a `Transport`) — must
//! uphold the same production guarantees. A new transport (or a change to one)
//! is held to this checklist:
//!
//! 1. **Trust boundary.** Client input passes
//!    [`meta::sanitize_inbound`](turbomcp_core::meta::sanitize_inbound) before
//!    any internal `_meta` key (`connectionId`, `sessionId`, `identity`) is
//!    injected — a client can never forge them. The serve driver does this for
//!    `Transport`-based servers; HTTP does it in its endpoint.
//! 2. **Authentication seam.** Where the deployment is network-reachable, the
//!    [`HttpAuthenticator`](crate::HttpAuthenticator) seam validates a bearer
//!    credential (per request on HTTP; at the upgrade for WebSocket, with the
//!    principal carried per-connection via [`ServeConfig::identity`](crate::ServeConfig)).
//!    stdio is a trusted local channel (a launcher owns both pipe ends); use a
//!    network transport when the deployment is reachable.
//! 3. **Cross-origin defense.** Browser-reachable endpoints validate `Origin`
//!    (HTTP requests, WebSocket upgrades) — default-deny with an allowlist.
//! 4. **Bounded input.** A size cap on inbound payloads: HTTP body limit, WS
//!    message limit, and the stdio line transport's per-frame cap
//!    (`LineTransport::with_max_line_bytes`, defaulting to
//!    `DEFAULT_MAX_LINE_BYTES`) — a peer that never sends `\n` is refused, not
//!    buffered without bound.
//! 5. **Liveness + shutdown.** Long-lived channels keep intermediaries alive
//!    (SSE keep-alive comments, WS idle pings) *and* reap peers that go silent
//!    (WS closes after `max_idle_pings` unanswered probes), and honor the
//!    shutdown token: accept loops stop, in-flight handlers drain within the
//!    configured deadline, `subscriptions/listen` streams close gracefully.
//! 6. **Backpressure.** Inbound dispatch is bounded (`max_in_flight` in the
//!    serve driver; connection/request limits in the HTTP stack) so a fast
//!    peer cannot grow memory without bound.
//! 7. **TLS.** Terminate at the ingress/proxy, or layer a TLS stream under the
//!    transport (`WebSocketTransport::accept` / `LineTransport::new` take any
//!    byte stream). No transport hand-rolls crypto.

use core::future::Future;
use std::time::Instant;

use turbomcp_core::JsonRpcMessage;

/// A bidirectional channel for JSON-RPC frames.
///
/// `recv` returns `Ok(None)` on a clean end-of-stream (peer closed); `Err` is
/// reserved for genuine I/O failure. `close` consumes the transport.
///
/// # Cancel safety
///
/// **[`recv`](Transport::recv) must be cancel safe.** Both drivers — the
/// server's `serve` loop and the client's connection actor — poll it as one
/// branch of a [`tokio::select!`] inside a loop, racing it against outbound
/// writes, handler completions, and the shutdown signal. Every time another
/// branch wins, the `recv` future is dropped part-way through and a fresh one
/// is created on the next turn. Dropping it must therefore lose nothing: any
/// bytes already taken from the underlying source have to live in the transport
/// so the next call resumes on top of them.
///
/// The practical rule is that the future may borrow state but must not *own*
/// any that matters. A partial frame kept in a local is gone when the future is
/// dropped; the same partial frame kept in `&mut self` survives. Getting this
/// wrong does not fail loudly — it silently truncates one frame, which then
/// fails to decode and takes the whole connection down with it, under load and
/// only under load.
///
/// The bundled transports satisfy this by construction: the stdio line reader
/// accumulates into a field, the WebSocket transport defers to `StreamExt::next`
/// on the underlying stream, and the HTTP client transport reads from an
/// `mpsc::Receiver`.
pub trait Transport: Send + 'static {
    /// Transport-specific failure (I/O, protocol framing).
    type Error: core::error::Error + Send + Sync + 'static;

    /// Send one frame to the peer.
    fn send(&mut self, msg: JsonRpcMessage)
    -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Receive the next frame, or `None` at clean end-of-stream.
    ///
    /// Must be cancel safe — see the [trait docs](Transport#cancel-safety).
    fn recv(&mut self) -> impl Future<Output = Result<Option<JsonRpcMessage>, Self::Error>> + Send;

    /// Close the transport, flushing anything pending.
    fn close(self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Close the transport, having been given a `deadline` by which any drain of
    /// pending writes should complete (PLAN §4.13).
    ///
    /// The default flushes-and-closes via [`Transport::close`], ignoring the
    /// deadline — correct for transports whose `send` already flushes each frame
    /// (e.g. stdio's line writer). Transports that buffer or own a long-lived
    /// outbound stream (HTTP SSE, in Phase 6) override this to honor the bound.
    fn graceful_shutdown(
        self,
        _deadline: Instant,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send
    where
        Self: Sized,
    {
        self.close()
    }
}
