//! The [`Transport`] trait: a bidirectional [`JsonRpcMessage`] channel.
//!
//! Transports own framing (line-delimited for stdio, SSE-event-framed for HTTP)
//! and hand the codec complete frames. The serve loop drives a `Transport`
//! directly, so the trait uses return-position `impl Future` (native AFIT/RPITIT)
//! rather than boxed futures — no per-message allocation, fully monomorphized.
//!
//! # The production parity contract
//!
//! Every bundled server transport — stdio ([`io::LineTransport`](crate::io::LineTransport)),
//! WebSocket (`turbomcp-transport-ws`), and Streamable HTTP
//! (`turbomcp-transport-http`, a runner rather than a `Transport`) — must
//! uphold the same production guarantees. A new transport (or a change to one)
//! is held to this checklist:
//!
//! 1. **Trust boundary.** What the transport knows about a request (its
//!    connection, session, authenticated identity, and the [`Peer`](crate::Peer)
//!    that reaches the client) travels typed in the
//!    [`McpRequest`](turbomcp_core::McpRequest)'s extensions, never in the
//!    message, so a client has no way to assert it. The serve driver attaches
//!    these for `Transport`-based servers; HTTP does it in its endpoint.
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

use std::collections::BTreeMap;

use turbomcp_core::{Extensions, InvalidFrame, JsonRpcMessage, ProtocolVersion, RequestId};

/// A bidirectional channel for JSON-RPC frames.
///
/// `recv` returns `Ok(None)` on a clean end-of-stream (peer closed); `Err` is
/// a failure, fatal unless [`invalid_frame`](Transport::invalid_frame) says
/// it cost only one frame. `close` consumes the transport.
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

    /// Whether this transport guarantees one stable principal for cached results.
    fn allows_response_cache(&self) -> bool {
        true
    }

    /// Whether messages ride HTTP requests, so the header-level features of
    /// Streamable HTTP apply: `MCP-Protocol-Version`, `x-mcp-header`
    /// mirroring. Only Streamable HTTP says yes.
    fn carries_headers(&self) -> bool {
        false
    }

    /// What this transport observed locally about the request `id` when it
    /// answered it with a synthesized error (the HTTP status, a stream that
    /// ended early). Read beside the response, never from it, so a peer can't
    /// forge one through JSON-RPC data.
    fn take_failure(&mut self, _id: &RequestId) -> Option<TransportFailure> {
        None
    }

    /// Send one frame with facts for the transport alone: the revision it goes
    /// out under ([`WireVersion`]), the `Mcp-Param-*` mirrors ([`ParamHeaders`]).
    /// A transport with nowhere to put them ignores them, which is the
    /// default.
    fn send_with(
        &mut self,
        msg: JsonRpcMessage,
        _facts: Extensions,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.send(msg)
    }

    /// Classify a [`recv`](Transport::recv) error: `Ok` means one frame was
    /// bad but the stream is intact, so the driver answers the frame (see
    /// [`InvalidFrame::response`]) and keeps reading. `Err` hands the error
    /// back as fatal.
    ///
    /// The default treats every error as fatal, which is right for a
    /// transport whose framing can't resynchronize. Newline-delimited stdio
    /// and message-framed WebSocket both can, and override this.
    ///
    /// # Errors
    /// Returns `error` unchanged when it is fatal.
    fn invalid_frame(error: Self::Error) -> Result<InvalidFrame, Self::Error>
    where
        Self: Sized,
    {
        Err(error)
    }

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
    /// deadline; the driver bounds the whole call by it anyway. Override it
    /// when the transport can do better than being cut off at the deadline
    /// (a WebSocket close handshake, say).
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

/// The revision an outbound message goes out under, for transports that say
/// so outside the message (Streamable HTTP's `MCP-Protocol-Version` header).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireVersion(pub ProtocolVersion);

/// The `x-mcp-header` mirrors for an outbound `tools/call`: header-name
/// portion to its already-encoded value, sent as `Mcp-Param-{name}`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamHeaders(pub BTreeMap<String, String>);

/// A failure a transport observed locally while answering a request.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum TransportFailure {
    /// The server answered with an HTTP error.
    #[error(transparent)]
    Http(HttpFailure),
    /// The response stream ended before the response arrived (a proxy idle
    /// timeout, a load balancer drain). The request may or may not have run.
    #[error("the response stream closed before the response arrived")]
    StreamLost,
}

/// Structured HTTP failure, preserving protocol errors and retry challenges.
#[derive(Debug, Clone, thiserror::Error)]
#[error("HTTP {status}: {message}")]
pub struct HttpFailure {
    /// HTTP response status.
    pub status: u16,
    /// Diagnostic summary, excluding bearer credentials.
    pub message: String,
    /// Parsed JSON-RPC error when the body contains one.
    pub rpc: Option<turbomcp_core::JsonRpcError>,
    /// Bearer challenge needed for discovery and scope escalation.
    pub www_authenticate: Option<String>,
    /// Server retry guidance, retained verbatim for caller policy.
    pub retry_after: Option<String>,
}
