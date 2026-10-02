//! The Streamable HTTP client transport (feature `client`).
//!
//! Streamable HTTP is request-scoped — each JSON-RPC request is its own POST —
//! yet the [`Connection`](turbomcp_client::Connection) actor wants the persistent
//! [`Transport`] `send`/`recv` shape. This transport bridges the two: `send`
//! fires a POST in a spawned task whose response (a single `application/json`
//! frame, or a `text/event-stream` of frames) is funneled into an inbound
//! channel that `recv` drains. Response correlation happens one layer up (by
//! request id in the `Connection`), so the transport never has to match
//! requests to responses itself — it only has to deliver every inbound frame.
//!
//! The `Mcp-Session-Id` minted by a legacy `initialize` response is captured
//! and replayed on subsequent requests (the stateful path); the stateless draft
//! path simply never sets it.
//!
//! Per-request failures are scoped to that request: a POST that fails (network,
//! HTTP status, decode) synthesizes a JSON-RPC error *response* for the
//! request's id rather than tearing down the whole connection — only the one
//! waiting caller sees the error.
//!
//! ## Cancellation is the disconnect
//!
//! On this transport, `notifications/cancelled` alone cannot stop a server: it
//! travels on a POST of its own, and a server scopes an in-flight request to
//! the POST that carried it. What the transports spec designates instead is
//! closing the request's response stream ("the server **MUST** treat a client
//! disconnect as cancellation of that request"), which for us means dropping
//! the POST task. So [`send`](Transport::send) reads the cancellations passing
//! through it and aborts the task it names — turning the portable signal the
//! [`Connection`](turbomcp_client::Connection) emits into the one HTTP actually listens
//! for.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use tokio::sync::{mpsc, watch};
use turbomcp_core::{
    CancellationToken, Extensions, JsonRpcError, JsonRpcMessage, JsonRpcResponse, ProtocolVersion,
    RequestId,
};
use turbomcp_protocol::methods::{notification, request};
use turbomcp_service::{
    HttpFailure, ParamHeaders, Transport, TransportFailure, WireVersion, mcp_headers,
};
pub use zeroize::Zeroizing;

use turbomcp_client::{Client, ClientBuilder, ClientError, ClientResult};

use crate::headers;

#[cfg(feature = "websocket")]
mod websocket;
#[cfg(feature = "websocket")]
pub use websocket::{WebSocketClientTransport, connect_websocket};

/// Failures specific to the HTTP client transport.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpClientError {
    /// The transport's inbound channel closed (connection torn down).
    #[error("http client transport closed")]
    Closed,
    /// The underlying `reqwest` client could not be built.
    #[error("http client build failed: {0}")]
    Build(#[source] reqwest::Error),
    /// A caller-supplied header is one the protocol owns (`Mcp-*`); the
    /// transport sets those itself.
    #[error("`{0}` is a protocol header; the transport sets it")]
    ReservedHeader(String),
}

/// A request's share of [`HttpClientLimits::max_posts`], held from admission
/// until its answer is handed over (`None` for notifications and responses,
/// which aren't counted).
type Slot = Option<tokio::sync::OwnedSemaphorePermit>;

/// Why a POST's pump stopped without delivering its response.
enum PumpFailure {
    /// The request failed: network, HTTP status, decode.
    Failed(String),
    /// The response stream ended before the response arrived.
    StreamLost,
}

impl From<String> for PumpFailure {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for PumpFailure {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_owned())
    }
}

/// Re-establish an expired session — once, however many requests noticed it.
///
/// Returns whether the caller should retry: `true` when a live session is now
/// in place (this call established it, or a concurrent one already had) and
/// `false` when there is no handshake to replay.
async fn recover_session(shared: &Arc<Shared>, sent: Option<&str>) -> Result<bool, String> {
    let _single_flight = shared.recovery.lock().await;
    // Whoever held the lock first may have replaced the session already, in
    // which case this request only needs re-sending against the new one.
    let current = shared.session.lock().expect("session mutex").clone();
    if let Some(current) = &current
        && Some(current.as_str()) != sent
    {
        return Ok(true);
    }
    let Some(handshake) = shared.handshake.lock().expect("handshake lock").clone() else {
        return Ok(false);
    };
    *shared.session.lock().expect("session mutex") = None;
    reinitialize(shared, &handshake).await?;
    Ok(true)
}

/// Re-establish an expired session by replaying the handshake, with no session
/// id attached.
///
/// Deliberately bypasses `pump`: this POST must not itself attempt session
/// recovery, and its response frames are the handshake's, which the typed
/// client above already saw the first time and must not see again. Only the
/// new `Mcp-Session-Id` is kept.
async fn reinitialize(shared: &Arc<Shared>, handshake: &JsonRpcMessage) -> Result<(), String> {
    let body = serde_json::to_string(handshake).map_err(|e| format!("encode failed: {e}"))?;
    let mut req = shared
        .http
        .post(&shared.url)
        .header(ACCEPT, "application/json, text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(source) = &shared.bearer
        && let Some(token) = source.bearer().await
    {
        req = req.bearer_auth(token.as_str());
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("re-initialize failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("re-initialize answered {}", resp.status()));
    }
    let Some(sid) = resp
        .headers()
        .get(headers::SESSION_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return Err("re-initialize established no session".into());
    };
    *shared.session.lock().expect("session mutex") = Some(sid.clone());
    // Drain the body so the connection can be reused; the frames are a repeat
    // of a handshake the client has already processed.
    let _ = bounded_body(resp, &shared.limits).await;
    // "After successful initialization, the client MUST send an `initialized`
    // notification." A new session is a new initialization, and a server may
    // refuse everything until it arrives.
    let initialized = serde_json::to_string(&JsonRpcMessage::Notification(
        turbomcp_core::JsonRpcNotification::new(notification::INITIALIZED, None),
    ))
    .map_err(|e| format!("encode failed: {e}"))?;
    let mut req = shared
        .http
        .post(&shared.url)
        .header(ACCEPT, "application/json, text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .header(headers::SESSION_ID, sid)
        .body(initialized);
    if let Some(version) = shared.version.lock().expect("version mutex").clone() {
        req = req.header(headers::PROTOCOL_VERSION, version);
    }
    if let Some(source) = &shared.bearer
        && let Some(token) = source.bearer().await
    {
        req = req.bearer_auth(token.as_str());
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("initialized notification failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "initialized notification answered {}",
            resp.status()
        ));
    }
    Ok(())
}

/// How long to wait before re-opening the standalone stream when the server
/// sends no `retry:` of its own.
const DEFAULT_SSE_RETRY: Duration = Duration::from_secs(1);

/// Maximum accepted server retry delay. Larger delays stop recovery; they
/// are never shortened, which would reconnect earlier than the server permits.
const MAX_SSE_RETRY: Duration = Duration::from_secs(30);

/// How long the handshake waits for the standalone stream to be established
/// before giving up on it and proceeding. Generous next to a loopback round
/// trip, and only ever paid once per connection.
const STREAM_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Supplies the bearer token for each outbound request.
///
/// Consulted per request rather than captured once, because that is what OAuth
/// needs: access tokens are short-lived by design, and a token refreshed out of
/// band has to take effect on the next request without rebuilding the transport
/// and re-running the handshake.
///
/// The token travels as [`Zeroizing`], wiped once the request has used it.
/// (The HTTP stack's own copy, in a header it marks sensitive, is beyond its
/// reach.)
#[async_trait::async_trait]
pub trait BearerSource: Send + Sync + 'static {
    /// The token to present, or `None` to send this request unauthenticated.
    async fn bearer(&self) -> Option<Zeroizing<String>>;

    /// Handle an HTTP authorization rejection. Return true to retry with the
    /// updated credential. At most three challenge retries occur per POST.
    /// `rejected` is the exact token used on that attempt; never log it.
    async fn on_challenge(
        &self,
        _status: u16,
        _header: Option<&str>,
        _rejected: Option<&str>,
    ) -> Result<bool, String> {
        Ok(false)
    }
}

/// A token that never changes.
#[async_trait::async_trait]
impl BearerSource for Zeroizing<String> {
    async fn bearer(&self) -> Option<Zeroizing<String>> {
        Some(self.clone())
    }
}

/// A token that can be replaced in place — the shape a refresh loop wants.
#[async_trait::async_trait]
impl BearerSource for Mutex<Option<Zeroizing<String>>> {
    async fn bearer(&self) -> Option<Zeroizing<String>> {
        self.lock().expect("bearer mutex poisoned").clone()
    }
}

/// HTTP transport resource budgets. Applied before connecting the transport.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct HttpClientLimits {
    /// Maximum concurrent POSTs, including their response streams.
    pub max_posts: usize,
    /// Maximum JSON response body or individual SSE event in bytes.
    pub max_response_bytes: usize,
    /// Deadline for a non-streaming response body.
    pub body_timeout: Duration,
}
impl Default for HttpClientLimits {
    fn default() -> Self {
        Self {
            max_posts: 1024,
            // The same ceiling stdio uses for a line: a screenshot or a large
            // `resources/read` works over one transport and failed at 1 MiB on
            // the other.
            max_response_bytes: 64 * 1024 * 1024,
            body_timeout: Duration::from_secs(30),
        }
    }
}

/// Shared state for the spawned POST tasks: the HTTP client, target URL, the
/// captured session id, the last negotiated protocol version, and the inbound
/// delivery channel.
struct Shared {
    http: reqwest::Client,
    limits: HttpClientLimits,
    url: String,
    session: Mutex<Option<String>>,
    /// The `initialize` frame this connection handshook with, kept so an
    /// expired session can be replaced the way the spec requires: a *new*
    /// `InitializeRequest` with no session id attached. Replaying the original
    /// re-negotiates identically, so the typed client's cached view of the
    /// handshake stays true.
    handshake: Mutex<Option<JsonRpcMessage>>,
    /// Serializes session recovery. Several POSTs can be in flight against one
    /// expired session and all of them 404; each re-handshaking independently
    /// would mint a session per concurrent request, leaving every one but the
    /// last talking to a session the server has already replaced.
    recovery: tokio::sync::Mutex<()>,
    /// The negotiated protocol version last seen on an outbound signal —
    /// the `MCP-Protocol-Version` header fallback for messages that carry no
    /// signal of their own (responses to server requests, notifications).
    version: Mutex<Option<String>>,
    inbound_tx: mpsc::Sender<JsonRpcMessage>,
    /// Whether the standalone server→client stream ([`listen`]) has been
    /// started. One per connection, however many POSTs race to trigger it.
    listening: AtomicBool,
    /// Where the `Authorization` header comes from, when the server wants one.
    bearer: Option<Arc<dyn BearerSource>>,
    /// The POST behind each in-flight request, so a cancellation can drop the
    /// one it names. Every task clears its own entry on the way out, so this
    /// holds only genuinely in-flight requests.
    posts: Mutex<HashMap<RequestId, CancellationToken>>,
    /// Why a request's POST failed, for the connection to read when the
    /// synthesized error response for it arrives.
    failures: Mutex<HashMap<RequestId, TransportFailure>>,
    shutdown: CancellationToken,
    tasks: tokio_util::task::TaskTracker,
    permits: Arc<tokio::sync::Semaphore>,
    /// Flips true once [`listen`] has an answer from the server. The handshake
    /// waits on it so a caller never holds a client whose server has nowhere to
    /// push (see [`await_stream_ready`]).
    stream_ready: watch::Sender<bool>,
}

impl Shared {
    /// Drop the POST carrying `id`, closing its response stream. That is what a
    /// server reads as cancellation on this transport. A request that already
    /// finished has no entry, which is the "cancelled too late" race the spec
    /// requires both sides to tolerate.
    fn abort_post(&self, id: &RequestId) {
        self.failures.lock().expect("failure lock").remove(id);
        if let Some(token) = self.posts.lock().expect("posts mutex poisoned").remove(id) {
            token.cancel();
        }
    }

    /// Deregister a finished POST.
    fn finish_post(&self, id: &RequestId) {
        self.posts.lock().expect("posts mutex poisoned").remove(id);
    }

    /// Attach `Authorization: Bearer …` when a token is available.
    async fn authorize(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.bearer {
            Some(source) => match source.bearer().await {
                Some(token) => req.bearer_auth(token.as_str()),
                None => req,
            },
            None => req,
        }
    }
}

/// A Streamable HTTP transport to a single MCP endpoint URL.
pub struct HttpClientTransport {
    shared: Arc<Shared>,
    inbound_rx: mpsc::Receiver<JsonRpcMessage>,
}

impl Drop for HttpClientTransport {
    fn drop(&mut self) {
        self.shared.shutdown.cancel();
        self.shared.tasks.close();
    }
}

async fn bounded_body(
    mut response: reqwest::Response,
    limits: &HttpClientLimits,
) -> Result<Vec<u8>, String> {
    tokio::time::timeout(limits.body_timeout, async {
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if chunk.len() > limits.max_response_bytes.saturating_sub(body.len()) {
                return Err("HTTP body limit exceeded".into());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    })
    .await
    .map_err(|_| "HTTP body deadline exceeded".to_owned())?
}
fn bounded_sse(
    response: reqwest::Response,
    max_bytes: usize,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
    let mut size = 0usize;
    let mut line_has_data = false;
    response.bytes_stream().map(move |chunk| {
        let chunk = chunk.map_err(std::io::Error::other)?;
        for &b in &chunk {
            size += 1;
            if size > max_bytes {
                return Err(std::io::Error::other("SSE event limit exceeded"));
            }
            if b == b'\n' {
                if !line_has_data {
                    size = 0;
                }
                line_has_data = false;
            } else if b != b'\r' {
                line_has_data = true;
            }
        }
        Ok(chunk)
    })
}

impl fmt::Debug for HttpClientTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the bearer token, and never the session id: one is a
        // credential outright and the other is a bearer-equivalent handle to
        // an authenticated session (transports spec: session ids MUST be
        // treated as secrets).
        f.debug_struct("HttpClientTransport")
            .field("url", &self.shared.url)
            .field("authenticated", &self.shared.bearer.is_some())
            .field(
                "session",
                &self
                    .shared
                    .session
                    .lock()
                    .map_or("<poisoned>", |s| if s.is_some() { "<set>" } else { "none" }),
            )
            .field(
                "in_flight_posts",
                &self.shared.posts.lock().map(|p| p.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl HttpClientTransport {
    /// Build a transport targeting `url` (e.g. `http://127.0.0.1:8080/mcp`).
    ///
    /// # Errors
    /// [`HttpClientError::Build`] if the underlying HTTP client can't be built.
    pub fn new(url: impl Into<String>) -> Result<Self, HttpClientError> {
        let http = default_http_client(reqwest::header::HeaderMap::new())?;
        let (inbound_tx, inbound_rx) = mpsc::channel(1024);
        Ok(Self {
            shared: Arc::new(Shared {
                http,
                limits: HttpClientLimits::default(),
                url: url.into(),
                session: Mutex::new(None),
                handshake: Mutex::new(None),
                recovery: tokio::sync::Mutex::new(()),
                version: Mutex::new(None),
                inbound_tx,
                listening: AtomicBool::new(false),
                bearer: None,
                posts: Mutex::new(HashMap::new()),
                failures: Mutex::new(HashMap::new()),
                shutdown: CancellationToken::new(),
                tasks: tokio_util::task::TaskTracker::new(),
                permits: Arc::new(tokio::sync::Semaphore::new(1024)),
                stream_ready: watch::channel(false).0,
            }),
            inbound_rx,
        })
    }

    /// Send every request through `http` — a client you built with your own
    /// proxy, root certificates, client certificate (mTLS), keepalive or
    /// default headers. The default client follows no redirects, which keeps a
    /// bearer token on the origin it was issued for; build yours the same way.
    ///
    /// # Panics
    /// If the transport is already connected. Call this first.
    #[must_use]
    pub fn with_client(mut self, http: reqwest::Client) -> Self {
        Arc::get_mut(&mut self.shared)
            .expect("with_client must be called before the transport is connected")
            .http = http;
        self
    }

    /// Add `headers` to every request this transport sends — POSTs, the
    /// standalone stream, session recovery and the closing `DELETE` — for the
    /// API keys, tenant and routing headers gateways ask for. Replaces any
    /// client given to [`with_client`](Self::with_client).
    ///
    /// # Errors
    /// [`HttpClientError::ReservedHeader`] for an `Mcp-*` header, which the
    /// protocol owns, or [`HttpClientError::Build`] if the client cannot be
    /// rebuilt.
    ///
    /// # Panics
    /// If the transport is already connected. Call this first.
    pub fn with_headers(
        self,
        headers: reqwest::header::HeaderMap,
    ) -> Result<Self, HttpClientError> {
        if let Some(reserved) = headers
            .keys()
            .find(|name| name.as_str().starts_with("mcp-"))
        {
            return Err(HttpClientError::ReservedHeader(reserved.to_string()));
        }
        let http = default_http_client(headers)?;
        Ok(self.with_client(http))
    }

    /// Configure budgets before handing the transport to a connection.
    #[must_use]
    pub fn with_limits(mut self, limits: HttpClientLimits) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configure limits before connecting");
        shared.permits = Arc::new(tokio::sync::Semaphore::new(limits.max_posts.max(1)));
        shared.limits = limits;
        self
    }

    /// Present `token` as `Authorization: Bearer …` on every request.
    ///
    /// For a token that changes — the usual case, since OAuth access tokens
    /// expire — use [`with_bearer_source`](Self::with_bearer_source) instead.
    #[must_use]
    pub fn with_bearer(self, token: impl Into<String>) -> Self {
        self.with_bearer_source(Arc::new(Zeroizing::new(token.into())))
    }

    /// Take the `Authorization` credential from `source`, which is consulted
    /// once per request so a refreshed token applies without reconnecting.
    ///
    /// This is the seam between [`turbomcp-auth`'s client OAuth
    /// flow](https://docs.rs/turbomcp-auth) and an authenticated session:
    /// the flow yields a `TokenSet`, and a source over it is what actually
    /// gets those tokens onto the wire.
    ///
    /// # Panics
    /// If another transport clone is mid-request. Call this before connecting.
    #[must_use]
    pub fn with_bearer_source(mut self, source: Arc<dyn BearerSource>) -> Self {
        Arc::get_mut(&mut self.shared)
            .expect("with_bearer_source must be called before the transport is connected")
            .bearer = Some(source);
        self
    }
}

/// The client [`HttpClientTransport::new`] uses: no redirects (a bearer token
/// stays on its origin) and a bounded connect.
fn default_http_client(
    headers: reqwest::header::HeaderMap,
) -> Result<reqwest::Client, HttpClientError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .default_headers(headers)
        .build()
        .map_err(HttpClientError::Build)
}

impl Transport for HttpClientTransport {
    type Error = HttpClientError;

    fn allows_response_cache(&self) -> bool {
        self.shared.bearer.is_none()
    }

    /// The negotiated version becomes `MCP-Protocol-Version` and the
    /// `#[mcp_header]` mirrors become `Mcp-Param-*`.
    fn carries_headers(&self) -> bool {
        true
    }

    fn take_failure(&mut self, id: &RequestId) -> Option<TransportFailure> {
        self.shared
            .failures
            .lock()
            .expect("failure lock")
            .remove(id)
    }

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        self.send_with(msg, Extensions::new()).await
    }

    async fn send_with(
        &mut self,
        msg: JsonRpcMessage,
        facts: Extensions,
    ) -> Result<(), Self::Error> {
        // Dropping the POST stops this client waiting on either wire. Whether
        // that also *cancels* is where the two revisions disagree, and the
        // disagreement is explicit on both sides:
        //
        // - `2026-07-28` Streamable HTTP: "closing the SSE response stream is
        //   itself the cancellation signal and no `notifications/cancelled`
        //   message is expected." Sending one anyway is a notification the
        //   revision does not define over this transport.
        // - `2025-11-25`: "Disconnection SHOULD NOT be interpreted as the
        //   client cancelling its request." Dropping the POST cancels nothing
        //   there, so the notification has to go out or the server keeps
        //   working on an answer no one will read.
        if let JsonRpcMessage::Notification(n) = &msg
            && n.method == notification::CANCELLED
            && let Some(id) = cancelled_request_id(n.params.as_ref())
        {
            self.shared.abort_post(&id);
            let stream_close_cancels = self
                .shared
                .version
                .lock()
                .expect("version mutex")
                .as_deref()
                .is_none_or(|v| v == ProtocolVersion::V2026_07_28.as_str());
            if stream_close_cancels {
                return Ok(());
            }
        }
        // Remember the handshake so an expired session can be re-established
        // without the typed client above having to know sessions exist.
        if let JsonRpcMessage::Request(r) = &msg
            && r.method == request::INITIALIZE
        {
            *self.shared.handshake.lock().expect("handshake lock") = Some(msg.clone());
        }

        // Learn the negotiated version here, in send order, not in the spawned
        // task: a task that has not run yet has not recorded it, so a DELETE
        // straight after the handshake, or a request racing the `initialized`
        // notification, went out without the version it was owed.
        let version = resolve_protocol_version(
            &msg,
            facts.get::<WireVersion>().map(|v| v.0.clone()),
            &self.shared,
        );
        let mirrors = facts.get::<ParamHeaders>().cloned().unwrap_or_default();

        // POST and pump the response in the background so the driver can keep
        // sending; HTTP requests are independent and may run concurrently.
        let shared = Arc::clone(&self.shared);
        // Registered *before* the task exists, so a POST that finishes
        // instantly cannot have its entry outlive it — the task clears the
        // entry itself, and can only run after this insert.
        let cancel = match &msg {
            JsonRpcMessage::Request(r) => {
                let token = CancellationToken::new();
                shared
                    .posts
                    .lock()
                    .expect("posts mutex poisoned")
                    .insert(r.id.clone(), token.clone());
                Some((r.id.clone(), token))
            }
            _ => None,
        };
        // Admission fails the one request that did not fit, never the
        // connection: treating it as a transport failure used to end the whole
        // client, and every in-flight call with it. Notifications and responses
        // are small and have no one waiting, so they are not held to the cap.
        let permit = match Arc::clone(&shared.permits).try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => match cancel {
                None => None,
                Some((id, _)) => {
                    shared.finish_post(&id);
                    let refused = JsonRpcResponse::error(
                        id,
                        JsonRpcError {
                            code: turbomcp_core::codes::SERVER_ERROR,
                            message: format!(
                                "too many concurrent requests on this HTTP client (max_posts = {})",
                                shared.limits.max_posts
                            ),
                            data: None,
                        },
                    );
                    let _ = shared
                        .inbound_tx
                        .send(JsonRpcMessage::Response(refused))
                        .await;
                    return Ok(());
                }
            },
        };
        let shutdown = shared.shutdown.clone();
        self.shared.tasks.spawn(async move {
            tokio::select! {
                () = shutdown.cancelled() => {}
                () = post_and_pump(shared, msg, version, mirrors, cancel, permit) => {}
            }
        });
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        // `None` here means every sender (the Shared in this transport + any
        // in-flight pump task) has dropped — a clean end-of-stream.
        Ok(self.inbound_rx.recv().await)
    }

    async fn close(self) -> Result<(), Self::Error> {
        self.shared.shutdown.cancel();
        self.shared.tasks.close();
        self.shared.tasks.wait().await;
        // Best-effort session termination (the spec's explicit DELETE).
        let sid = self.shared.session.lock().expect("session mutex").clone();
        if let Some(sid) = sid {
            let mut req = self
                .shared
                .http
                .delete(&self.shared.url)
                .header(headers::SESSION_ID, sid)
                .timeout(Duration::from_secs(5));
            // "the client MUST include the `MCP-Protocol-Version` header on all
            // subsequent requests" — a strict server refuses the DELETE
            // without it, and the session outlives the client.
            if let Some(version) = self.shared.version.lock().expect("version mutex").clone() {
                req = req.header(headers::PROTOCOL_VERSION, version);
            }
            // Authenticated too: on a server that requires a bearer, an
            // unauthenticated DELETE is a 401 and the session leaks.
            let _ = self.shared.authorize(req).await.send().await;
        }
        Ok(())
    }
}

/// POST `msg` and feed whatever comes back into the inbound channel.
///
/// `cancel` is present for requests: its token fires when the caller abandons
/// the request, which drops the `pump` future and with it the response stream —
/// the disconnect a server reads as cancellation. The registration is cleared
/// on every exit path, so the map holds only in-flight POSTs.
///
/// `slot` is released before the answer is handed over, never after: a caller
/// holding its response can send its next request at once, and releasing
/// after delivery let that request find the slot still taken and be refused.
async fn post_and_pump(
    shared: Arc<Shared>,
    msg: JsonRpcMessage,
    version: Option<String>,
    mirrors: ParamHeaders,
    cancel: Option<(RequestId, CancellationToken)>,
    mut slot: Slot,
) {
    // The request id (if this is a request) so a failure can be reported to just
    // this caller as an error response rather than killing the connection.
    let request_id = match &msg {
        JsonRpcMessage::Request(r) => Some(r.id.clone()),
        _ => None,
    };

    let outcome = match &cancel {
        Some((_, token)) => tokio::select! {
            // Abandoned: drop the POST without synthesizing an answer. The
            // caller has already stopped waiting, and `Connection` has taken
            // its pending entry.
            () = token.cancelled() => None,
            result = pump(&shared, msg, version, mirrors, &mut slot) => Some(result),
        },
        None => Some(pump(&shared, msg, version, mirrors, &mut slot).await),
    };
    drop(slot);
    if let Some((id, _)) = &cancel {
        shared.finish_post(id);
    }
    let Some(result) = outcome else { return };

    if let Err(failure) = result {
        let message = match failure {
            PumpFailure::Failed(message) => message,
            PumpFailure::StreamLost => {
                if let Some(id) = &request_id {
                    shared
                        .failures
                        .lock()
                        .expect("failure lock")
                        .insert(id.clone(), TransportFailure::StreamLost);
                }
                TransportFailure::StreamLost.to_string()
            }
        };
        match request_id {
            Some(id) => {
                // Surface the failure to the one waiting caller. This is a
                // locally synthesized *transport* error, so it takes the
                // implementation-defined floor — the same code
                // `McpError::Transport` maps to.
                let resp = JsonRpcResponse::error(
                    id,
                    JsonRpcError {
                        code: turbomcp_core::codes::SERVER_ERROR,
                        message,
                        data: None,
                    },
                );
                let _ = shared.inbound_tx.send(JsonRpcMessage::Response(resp)).await;
            }
            // Notifications / responses have no waiter — just log.
            None => tracing::debug!(error = %message, "http client POST failed (no waiter)"),
        }
    }
}

/// The `requestId` a `notifications/cancelled` names, as a [`RequestId`].
/// A malformed notification names nothing and cancels nothing.
fn cancelled_request_id(params: Option<&serde_json::Value>) -> Option<RequestId> {
    serde_json::from_value(params?.get("requestId")?.clone()).ok()
}

/// The fallible body of a POST + response pump. Errors are returned as a string
/// for [`post_and_pump`] to route.
async fn pump(
    shared: &Arc<Shared>,
    msg: JsonRpcMessage,
    version: Option<String>,
    mirrors: ParamHeaders,
    slot: &mut Slot,
) -> Result<(), PumpFailure> {
    let body = serde_json::to_string(&msg).map_err(|e| format!("encode failed: {e}"))?;

    let mut req = shared
        .http
        .post(&shared.url)
        .header(ACCEPT, "application/json, text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .body(body);
    // The session header is applied per *attempt*, not baked into the
    // template: a 404 means the session is gone, and the retry after
    // re-handshaking has to carry the new id rather than the dead one.
    // `MCP-Protocol-Version` is required on every POST (both versions'
    // transports specs; on `2025-11-25` from the first post-`initialize`
    // request onward — the handshake itself negotiates in-band).
    if let Some(v) = &version {
        req = req.header(headers::PROTOCOL_VERSION, v);
    }
    // The draft's standard request headers mirror body fields for
    // intermediaries: `Mcp-Method` on every request POST, `Mcp-Name` for
    // `tools/call`/`resources/read`/`prompts/get` and the Tasks extension's
    // `tasks/*` (its task id, for routing). `2025-11-25` doesn't define them.
    let is_draft = version
        .as_deref()
        .is_some_and(|v| ProtocolVersion::from_wire(v) == ProtocolVersion::V2026_07_28);
    if is_draft && let JsonRpcMessage::Request(r) = &msg {
        req = req.header(headers::MCP_METHOD, &r.method);
        if let Some(field) = mcp_headers::routing_name_field(&r.method)
            && let Some(value) = r
                .params
                .as_ref()
                .and_then(|p| p.get(field))
                .and_then(serde_json::Value::as_str)
        {
            req = req.header(headers::MCP_NAME, mcp_headers::encode_value(value));
        }
    }
    for (name, value) in mirrors.0 {
        req = req.header(format!("{}{name}", headers::MCP_PARAM_PREFIX), value);
    }

    let mut attempts = 0;
    let resp = loop {
        let token = match &shared.bearer {
            Some(source) => source.bearer().await,
            None => None,
        };
        let mut attempt = req.try_clone().ok_or("HTTP request cannot be retried")?;
        if let Some(token) = &token {
            attempt = attempt.bearer_auth(token.as_str());
        }
        let sent_session = shared.session.lock().expect("session mutex").clone();
        if let Some(sid) = &sent_session {
            attempt = attempt.header(headers::SESSION_ID, sid);
        }
        let resp = attempt
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            break resp;
        }
        let www_authenticate = resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = bounded_body(resp, &shared.limits).await;
        let rpc = body
            .as_ref()
            .ok()
            .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
            .and_then(|v| v.get("error").cloned())
            .and_then(|v| serde_json::from_value(v).ok());
        let mut message = format!("http status {status}");
        // "When a client receives HTTP 404 in response to a request containing
        // an `Mcp-Session-Id`, it MUST start a new session by sending a new
        // `InitializeRequest` without a session ID attached." Clearing the id
        // alone is not enough — the next POST would go out sessionless and be
        // refused as uninitialized — so the handshake this transport already
        // watched go past is replayed, and the original request retried once
        // against the session that establishes.
        if status.as_u16() == 404 && sent_session.is_some() && attempts < 3 {
            match recover_session(shared, sent_session.as_deref()).await {
                Ok(true) => {
                    attempts += 1;
                    continue;
                }
                // Nothing to replay: this connection never ran a handshake, so
                // the 404 is the answer.
                Ok(false) => {}
                Err(e) => {
                    message = format!("{message}: session expired, re-initialize failed: {e}")
                }
            }
        }
        if matches!(status.as_u16(), 401 | 403)
            && attempts < 3
            && let Some(source) = &shared.bearer
        {
            match source
                .on_challenge(
                    status.as_u16(),
                    www_authenticate.as_deref(),
                    token.as_deref().map(String::as_str),
                )
                .await
            {
                Ok(true) => {
                    attempts += 1;
                    continue;
                }
                Ok(false) => {}
                Err(error) => message = format!("{message}: authorization failed: {error}"),
            }
        }
        if let JsonRpcMessage::Request(r) = &msg {
            shared.failures.lock().expect("failure lock").insert(
                r.id.clone(),
                TransportFailure::Http(HttpFailure {
                    status: status.as_u16(),
                    message: message.clone(),
                    rpc,
                    www_authenticate,
                    retry_after,
                }),
            );
        }
        return Err(message.into());
    };
    // Only successful responses can establish or replace a session.
    if let Some(sid) = resp
        .headers()
        .get(headers::SESSION_ID)
        .and_then(|v| v.to_str().ok())
    {
        *shared.session.lock().expect("session mutex") = Some(sid.to_string());
    }

    // The standalone stream belongs to the stateful transport, so open it as
    // soon as a negotiated stateful version proves we are on that path — see
    // [`listen`]. The earliest frame carrying one is the handshake's
    // `notifications/initialized`, so the stream is up before the client's
    // first request rather than waiting on one it may never make.
    // Either signal means this is a stateful connection: an outbound version
    // we have already negotiated, or the server minting a session id. The
    // second is what lets the *handshake itself* trigger this — a client has no
    // negotiated version yet when it POSTs `initialize`, so keying only on the
    // first opened the stream a POST too late.
    let stateful = version
        .as_deref()
        .map(ProtocolVersion::from_wire)
        .is_some_and(|v| v.is_stateful())
        || shared.session.lock().expect("session mutex").is_some();
    // The handshake's own stream opens once its answer is read, so that the
    // negotiated version is known and the stream's GET can carry it; any
    // other request opens it straight away.
    let is_initialize =
        matches!(&msg, JsonRpcMessage::Request(r) if r.method == request::INITIALIZE);
    if stateful && !is_initialize {
        ensure_listening(shared).await;
    }

    let is_sse = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/event-stream"));

    if is_sse {
        pump_sse(
            shared,
            resp,
            &msg,
            stateful && !is_draft,
            version.as_deref(),
            stateful && is_initialize,
            slot,
        )
        .await?;
    } else {
        // application/json: a single response frame (202 with no body → nothing).
        let text = String::from_utf8(bounded_body(resp, &shared.limits).await?)
            .map_err(|e| e.to_string())?;
        if text.trim().is_empty() {
            // Nothing to wait for after a notification or a response; a
            // request with no answer would otherwise hang until its timeout.
            return if matches!(msg, JsonRpcMessage::Request(_)) {
                Err(PumpFailure::StreamLost)
            } else {
                Ok(())
            };
        }
        let frame: JsonRpcMessage =
            serde_json::from_str(&text).map_err(|e| format!("json decode failed: {e}"))?;
        learn_negotiated_version(shared, &msg, &frame);
        if stateful && is_initialize {
            ensure_listening(shared).await;
        }
        slot.take();
        let _ = shared.inbound_tx.send(frame).await;
    }
    Ok(())
}

/// Record the version an `initialize` answer negotiated. It is the one frame
/// that states it outright, and it passes through here before the client sees
/// it; waiting for the client's `initialized` notification instead left the
/// version unknown until that notification's POST ran — so a `DELETE` right
/// after connecting, and the stream's first GET, went out without it.
fn learn_negotiated_version(shared: &Shared, sent: &JsonRpcMessage, frame: &JsonRpcMessage) {
    if let (JsonRpcMessage::Request(q), JsonRpcMessage::Response(r)) = (sent, frame)
        && q.method == request::INITIALIZE
        && r.id.as_ref() == Some(&q.id)
        && let Some(version) = r
            .result
            .as_ref()
            .and_then(|result| result.get("protocolVersion"))
            .and_then(serde_json::Value::as_str)
    {
        *shared.version.lock().expect("version mutex") = Some(version.to_owned());
    }
}

/// Open the standalone server→client stream, once per connection, and hold
/// the caller until the server has answered it — so a client handed back from
/// the handshake always has a stream behind it.
async fn ensure_listening(shared: &Arc<Shared>) {
    if shared.listening.swap(true, Ordering::AcqRel) {
        return;
    }
    let listen_shared = Arc::clone(shared);
    shared.tasks.spawn(async move {
        tokio::select! {
            () = listen_shared.shutdown.cancelled() => {}
            () = listen_shared.inbound_tx.closed() => {}
            () = listen(Arc::clone(&listen_shared)) => {}
        }
    });
    await_stream_ready(shared).await;
}

/// Resume a closed legacy response stream without replaying its POST. The
/// surrounding request task owns cancellation, admission, and the deadline.
async fn pump_sse(
    shared: &Arc<Shared>,
    mut response: reqwest::Response,
    request: &JsonRpcMessage,
    stateful: bool,
    version: Option<&str>,
    open_stream_on_answer: bool,
    slot: &mut Slot,
) -> Result<(), PumpFailure> {
    let mut cursor = None;
    let mut retry = DEFAULT_SSE_RETRY;
    loop {
        let mut events = bounded_sse(response, shared.limits.max_response_bytes).eventsource();
        while let Some(event) = events.next().await {
            let event = event.map_err(|e| format!("sse stream error: {e}"))?;
            if !event.id.is_empty() {
                cursor = Some(event.id);
            }
            if let Some(delay) = event.retry {
                retry = delay;
            }
            if event.data.is_empty() {
                continue;
            }
            let frame: JsonRpcMessage = serde_json::from_str(&event.data)
                .map_err(|e| format!("sse frame decode failed: {e}"))?;
            let finished = matches!(&frame, JsonRpcMessage::Response(r)
                if matches!(request, JsonRpcMessage::Request(q) if r.id.as_ref() == Some(&q.id)));
            if finished {
                learn_negotiated_version(shared, request, &frame);
                if open_stream_on_answer {
                    ensure_listening(shared).await;
                }
                slot.take();
            }
            if shared.inbound_tx.send(frame).await.is_err() || finished {
                return Ok(());
            }
        }
        // Only legacy request streams carrying a replay cursor are resumable.
        // Anything else that ends here has lost its response — unless there was
        // none to wait for.
        let Some(id) = cursor
            .as_deref()
            .filter(|_| stateful && matches!(request, JsonRpcMessage::Request(_)))
        else {
            return if matches!(request, JsonRpcMessage::Request(_)) {
                Err(PumpFailure::StreamLost)
            } else {
                Ok(())
            };
        };
        if retry > MAX_SSE_RETRY {
            return Err("SSE retry delay exceeds recovery limit".to_owned().into());
        }
        tokio::time::sleep(retry).await;
        let mut get = shared
            .http
            .get(&shared.url)
            .header(ACCEPT, "text/event-stream")
            .header(headers::LAST_EVENT_ID, id);
        if let Some(sid) = shared.session.lock().expect("session mutex").clone() {
            get = get.header(headers::SESSION_ID, sid);
        }
        if let Some(version) = version {
            get = get.header(headers::PROTOCOL_VERSION, version);
        }
        response = shared
            .authorize(get)
            .await
            .send()
            .await
            .map_err(|e| format!("SSE resume failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("SSE resume refused: {}", response.status()).into());
        }
        if !response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"))
        {
            return Err("SSE resume returned a non-SSE response".into());
        }
    }
}

/// Hold open the standalone server→client SSE stream: `GET` on the MCP
/// endpoint, re-opened for as long as the connection lives.
///
/// Streamable HTTP gives a server two places to put a message it originates:
/// inline on the SSE stream of whichever POST it is currently answering, or
/// here. Which one it picks is the server's choice and nothing on the wire
/// announces it — the reference TypeScript SDK uses this stream, while
/// TurboMCP's own server answers inline. A client that never issues the `GET`
/// therefore looks completely correct against some servers and silently hangs
/// against others: every `elicitation/create`, `sampling/createMessage`, and
/// `roots/list` delivered here would simply never arrive, and the server would
/// wait out its own timeout.
///
/// Re-opening is not error recovery. The server is expected to end this stream
/// and have the client come back (SEP-1699), so a graceful close is a reconnect
/// signal; `Last-Event-ID` asks the server to resume from the last event it
/// managed to deliver, and its `retry:` field sets the delay when it has an
/// opinion.
///
/// Offering the stream is optional for a server. One that answers `405` (or
/// `404`/`501`) is saying it has nothing to push, which is a complete answer,
/// so the loop stops rather than reconnecting forever.
async fn listen(shared: Arc<Shared>) {
    let mut last_event_id: Option<String> = None;
    let mut retry = DEFAULT_SSE_RETRY;
    // Consecutive refusals, for backoff: a GET the server keeps rejecting used
    // to be re-sent every second for the life of the client.
    let mut refusals = 0u32;

    loop {
        // The connection actor has gone; nobody is left to receive.
        if shared.inbound_tx.is_closed() {
            return;
        }

        let mut req = shared
            .http
            .get(&shared.url)
            .header(ACCEPT, "text/event-stream");
        let sent_session = shared.session.lock().expect("session mutex").clone();
        if let Some(sid) = &sent_session {
            req = req.header(headers::SESSION_ID, sid);
        }
        if let Some(v) = shared.version.lock().expect("version mutex").clone() {
            req = req.header(headers::PROTOCOL_VERSION, v);
        }
        if let Some(id) = &last_event_id {
            req = req.header(headers::LAST_EVENT_ID, id);
        }

        let outcome = shared.authorize(req).await.send().await;
        // The server has answered the GET, one way or another: the stream is
        // up, or it has said it offers none. Either resolves what
        // [`connect_http`] is waiting on. Setting it repeatedly on reconnects
        // is harmless — it is already `true`.
        let _ = shared.stream_ready.send(true);
        match outcome {
            Ok(resp) if resp.status().is_success() => {
                refusals = 0;
                let mut events = bounded_sse(resp, shared.limits.max_response_bytes).eventsource();
                while let Some(event) = events.next().await {
                    let Ok(event) = event else { break };
                    if !event.id.is_empty() {
                        last_event_id = Some(event.id.clone());
                    }
                    if let Some(server_retry) = event.retry {
                        retry = server_retry;
                    }
                    if event.data.is_empty() {
                        continue; // keep-alive / priming event
                    }
                    match serde_json::from_str::<JsonRpcMessage>(&event.data) {
                        // Closed receiver: the connection is gone for good.
                        Ok(frame) => {
                            if shared.inbound_tx.send(frame).await.is_err() {
                                return;
                            }
                        }
                        // One malformed frame is not a reason to abandon the
                        // only channel the server has for reaching us.
                        Err(e) => {
                            tracing::debug!(error = %e, "standalone sse frame decode failed");
                        }
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                // 405/501 mean this server has no standalone stream, which is
                // permanent. 404 on a session-bearing GET means the *session*
                // is gone, which is recoverable and must not be mistaken for
                // the former — doing so silently killed the server→client
                // channel for the rest of the connection.
                if matches!(status.as_u16(), 405 | 501) {
                    tracing::debug!(%status, "server does not offer a standalone sse stream");
                    return;
                }
                // "When a client receives HTTP 404 in response to a request
                // containing an `MCP-Session-Id`, it MUST start a new session."
                // Through the same single-flight path the POSTs use: clearing
                // the id here instead left every later POST sessionless, so
                // each got a 400 rather than the 404 that triggers recovery,
                // and the client stayed broken for good.
                if status.as_u16() == 404 && sent_session.is_some() {
                    match recover_session(&shared, sent_session.as_deref()).await {
                        Ok(true) => {
                            refusals = 0;
                            continue; // reopen at once, on the new session
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::debug!(error = %e, "standalone sse stream: session recovery failed");
                        }
                    }
                }
                refusals = refusals.saturating_add(1);
                tracing::debug!(%status, "standalone sse stream rejected; retrying");
            }
            Err(e) => {
                refusals = refusals.saturating_add(1);
                tracing::debug!(error = %e, "standalone sse stream failed; retrying");
            }
        }

        if retry > MAX_SSE_RETRY {
            tracing::debug!("standalone SSE retry exceeds recovery limit");
            return;
        }
        // Never sooner than the server asked; longer the more it refuses.
        let backoff = retry
            .saturating_mul(1 << refusals.min(5))
            .min(MAX_SSE_RETRY.max(retry));
        tokio::time::sleep(backoff).await;
    }
}

/// Resolve the `MCP-Protocol-Version` header value for `msg`: the revision
/// the client says it goes out under, else the body's `_meta` protocol version
/// (draft requests), else the last version seen on this connection (covers
/// responses to server requests, which the client sends without one).
/// Remembers whatever it resolves.
fn resolve_protocol_version(
    msg: &JsonRpcMessage,
    explicit: Option<ProtocolVersion>,
    shared: &Shared,
) -> Option<String> {
    // `initialize` *is* the negotiation: nothing has been agreed yet, so it
    // carries no version header and forgets any it inherited. After a failed
    // `server/discover` probe the remembered value was that probe's
    // `2026-07-28`, and the fallback `initialize` went out claiming it — a
    // strict legacy server refuses exactly that.
    if matches!(msg, JsonRpcMessage::Request(r) if r.method == request::INITIALIZE) {
        *shared.version.lock().expect("version mutex") = None;
        return None;
    }
    let version = explicit.map(|v| v.as_str().to_owned()).or_else(|| {
        match msg {
            JsonRpcMessage::Request(r) => r.params.as_ref(),
            JsonRpcMessage::Notification(n) => n.params.as_ref(),
            JsonRpcMessage::Response(_) => None,
        }?
        .get("_meta")?
        .get(turbomcp_core::meta::keys::PROTOCOL_VERSION)?
        .as_str()
        .map(str::to_owned)
    });

    let mut last = shared.version.lock().expect("version mutex");
    match version {
        Some(v) => {
            *last = Some(v.clone());
            Some(v)
        }
        None => last.clone(),
    }
}

/// Connect a [`Client`] to an MCP server over Streamable HTTP at `url`, running
/// the handshake.
///
/// # Errors
/// Propagates handshake failures; a transport that can't be built is
/// [`ClientError::Protocol`].
pub async fn connect_http(builder: ClientBuilder, url: impl Into<String>) -> ClientResult<Client> {
    let transport =
        HttpClientTransport::new(url).map_err(|e| ClientError::Protocol(e.to_string()))?;
    builder.connect(transport).await
}

/// Hold the handshake response until the standalone server→client stream has
/// been established (or the server has said it offers none).
///
/// Opening the stream is not the same as having opened it. It runs in a spawned
/// task, so without this the caller gets its client first and the stream races
/// to catch up. A server that pushes only on the standalone stream has nowhere
/// to deliver during that window, and the reference implementation does not
/// wait for us — it fails the request with `-32000 Connection closed`. Being a
/// race, it surfaces as an intermittent.
///
/// This lives in the transport rather than in [`connect_http`] because
/// attaching a bearer token means building the transport yourself and calling
/// [`ClientBuilder::connect`] directly, which is exactly the shape an
/// authenticated production client has — a check the common path skips is not
/// much of a check.
///
/// Bounded and never fatal. A server that will not answer the `GET` until the
/// handshake POST is fully drained would otherwise deadlock, and one that is
/// merely slow should not fail a connection that is otherwise fine — in both
/// cases the timeout leaves behaviour exactly as it was before this wait
/// existed.
async fn await_stream_ready(shared: &Arc<Shared>) {
    let mut ready = shared.stream_ready.subscribe();
    if *ready.borrow() {
        return;
    }
    match tokio::time::timeout(STREAM_READY_TIMEOUT, ready.changed()).await {
        Ok(Ok(())) => {}
        // The sender is gone, so no stream is coming; nothing to wait for.
        Ok(Err(_closed)) => {}
        Err(_elapsed) => tracing::debug!(
            "standalone sse stream not established within {STREAM_READY_TIMEOUT:?}; \
             continuing without it"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_core::JsonRpcRequest;

    #[test]
    fn the_stated_version_is_used_and_remembered() {
        let shared = Shared {
            http: reqwest::Client::new(),
            limits: HttpClientLimits::default(),
            url: "http://unused/mcp".into(),
            session: Mutex::new(None),
            handshake: Mutex::new(None),
            recovery: tokio::sync::Mutex::new(()),
            version: Mutex::new(None),
            inbound_tx: mpsc::channel(1).0,
            listening: AtomicBool::new(false),
            bearer: None,
            posts: Mutex::new(HashMap::new()),
            stream_ready: watch::channel(false).0,
            failures: Mutex::new(HashMap::new()),
            shutdown: CancellationToken::new(),
            tasks: tokio_util::task::TaskTracker::new(),
            permits: Arc::new(tokio::sync::Semaphore::new(1024)),
        };

        // A legacy request states its revision as a transport fact.
        let msg = JsonRpcMessage::Request(JsonRpcRequest::new(1, "tools/list", None));
        assert_eq!(
            resolve_protocol_version(&msg, Some(ProtocolVersion::V2025_11_25), &shared).as_deref(),
            Some("2025-11-25")
        );

        // A draft request carries it in the body's `_meta`.
        let draft = JsonRpcMessage::Request(JsonRpcRequest::new(
            2,
            "tools/list",
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } })),
        ));
        assert_eq!(
            resolve_protocol_version(&draft, None, &shared).as_deref(),
            Some("2026-07-28")
        );

        // A follow-up that states nothing (a response to a server request)
        // falls back to the remembered version.
        let response = JsonRpcMessage::Response(JsonRpcResponse::success(
            turbomcp_core::RequestId::from(3),
            json!({}),
        ));
        assert_eq!(
            resolve_protocol_version(&response, None, &shared).as_deref(),
            Some("2026-07-28")
        );
    }

    /// Both JSON-RPC id forms round-trip, since which one a cancellation names
    /// decides whether the right POST is dropped.
    #[test]
    fn a_cancellation_names_its_request_in_either_id_form() {
        assert_eq!(
            cancelled_request_id(Some(&json!({ "requestId": 7 }))),
            Some(RequestId::from(7))
        );
        assert_eq!(
            cancelled_request_id(Some(&json!({ "requestId": "abc" }))),
            Some(RequestId::from("abc"))
        );
    }

    /// A malformed cancellation must cancel nothing rather than guess.
    #[test]
    fn a_malformed_cancellation_names_nothing() {
        assert_eq!(cancelled_request_id(None), None);
        assert_eq!(cancelled_request_id(Some(&json!({}))), None);
        assert_eq!(
            cancelled_request_id(Some(&json!({ "requestId": { "not": "an id" } }))),
            None
        );
    }
}
