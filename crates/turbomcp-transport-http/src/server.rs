//! The server half: an axum endpoint that drives an [`McpService`].

mod config;
mod guards;
mod reject;
mod sse;
mod streams;
mod validate;
#[cfg(feature = "websocket")]
mod websocket;

pub use config::HttpConfig;
#[cfg(feature = "websocket")]
pub use websocket::WebSocketConfig;

use std::future::poll_fn;
use std::net::SocketAddr;

use ipnet::IpNet;
use std::sync::Arc;
use std::time::Duration;

use crate::headers;
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::serve::ListenerExt as _;
use serde_json::json;
use tokio_util::task::TaskTracker;
use turbomcp_core::codec::{Codec, DefaultCodec};
use turbomcp_core::{
    Extensions, JsonRpcMessage, McpRequest, ObservedHeaders, ProtocolVersion, SessionId, meta,
};
use turbomcp_service::{
    CancellationToken, HttpAuthenticator, McpService, Peer, ProtocolError, RateKey, RateLimiter,
    Serve, ServerHandle, SessionStreams, SessionTerminator, catch_handler_panic,
    close_then_shut_down,
};

use config::{HostPolicy, OriginPolicy, RESOURCE_METADATA_PATH};
use guards::{
    PeerIp, accepts, check_host, check_origin, client_key, enforce_auth, enforce_rate_limit,
};
use reject::{
    deadline_passed, envelope_rejection, invalid_frame_response, method_not_allowed,
    not_acceptable_rejection, protocol_error_response, service_unavailable, session_not_found,
    session_required_rejection, sessions_need_an_owner, too_many_requests,
    version_header_rejection,
};
use sse::{AbortOnDrop, Outlet, SSE_CHANNEL_CAPACITY, request_stream, sse_response};
use streams::{Admission, StreamBudget};
use validate::{declared_version, message_has_version, request_id, validate_request_headers};

/// Errors from running the HTTP transport.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpError {
    /// Binding the listener or running the server failed.
    #[error("http server i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Erase into the service-layer boundary error so a binary that serves over
/// more than one transport can `?` every entry point against a single
/// `Result<(), ProtocolError>` — the type `serve_stdio` already returns.
impl From<HttpError> for ProtocolError {
    fn from(err: HttpError) -> Self {
        match err {
            HttpError::Io(e) => ProtocolError::Transport(format!("http: {e}")),
        }
    }
}

/// Per-request shared state: the service to dispatch into, the codec, the
/// Origin policy, and the optional resource-server authenticator. Cheap to
/// clone (the service clones per request by contract).
#[derive(Clone)]
struct HttpState<S> {
    service: S,
    codec: DefaultCodec,
    origins: OriginPolicy,
    hosts: HostPolicy,
    sse_keepalive: Duration,
    sse_upgrade_after: Duration,
    authenticator: Option<Arc<dyn HttpAuthenticator>>,
    rate_limiter: Option<Arc<dyn RateLimiter>>,
    session_terminator: Option<Arc<dyn SessionTerminator>>,
    trusted_proxies: Arc<[IpNet]>,
    /// This endpoint's stateful sessions' `GET` streams, attached to every
    /// request so the dispatcher can reach a session's current stream.
    streams: SessionStreams,
    /// The revisions this endpoint serves, for the `MCP-Protocol-Version`
    /// check and the `supported` list its rejection carries.
    supported_versions: Arc<[ProtocolVersion]>,
    /// The configured shutdown token; long-lived streams end when it fires
    /// (for listen streams, the RC's server-side subscription close).
    shutdown: CancellationToken,
    /// What open long-lived streams may hold.
    stream_budget: StreamBudget,
    /// Every request's call, each on a task of its own; graceful shutdown
    /// waits for them.
    calls: TaskTracker,
}

impl<S> HttpState<S> {
    /// Whether this endpoint serves the revision a `MCP-Protocol-Version`
    /// header names.
    fn serves(&self, version: &str) -> bool {
        let requested = ProtocolVersion::from_wire(version);
        self.supported_versions.contains(&requested)
    }

    /// Whether this endpoint serves any revision with sessions. One that
    /// serves only `2026-07-28` has no `GET` stream and nothing to `DELETE`.
    fn serves_sessions(&self) -> bool {
        self.supported_versions
            .iter()
            .any(ProtocolVersion::is_stateful)
    }

    /// The `400` for a `MCP-Protocol-Version` this endpoint does not serve, on
    /// the verbs that carry no JSON-RPC id to name.
    ///
    /// The rule is "all subsequent requests", not "all POSTs": a `GET` opening
    /// the session's stream and a `DELETE` ending it are requests too, and a
    /// header naming a version the server never agreed to is as wrong there.
    fn reject_version_header(&self, headers: &HeaderMap) -> Option<Response> {
        let version = headers
            .get(&headers::PROTOCOL_VERSION)
            .and_then(|v| v.to_str().ok())?;
        (!self.serves(version))
            .then(|| version_header_rejection(None, version, &self.supported_versions))
    }
}

/// Build the configured axum [`Router`] for `server` without binding a socket —
/// the unit of composition (mount it under a larger app) and the seam tests
/// drive via `tower::ServiceExt::oneshot`.
///
/// `server` is usually a `turbomcp-server` `Server` (or a builder's
/// `.layer(…)`), which brings its supported revisions and its `DELETE`
/// handler. A bare service works too and brings neither.
///
/// Closing `subscriptions/listen` streams at shutdown is the listener's job,
/// not the router's: [`serve_http`] and [`Http`] do it. When you serve this
/// router yourself, call `ServerHandle::close_subscriptions` before firing
/// the configured shutdown token.
pub fn router<H: ServerHandle>(server: H, config: HttpConfig) -> Router {
    let supported_versions = config
        .supported_versions
        .clone()
        .unwrap_or_else(|| server.supported_versions());
    let session_terminator = config
        .session_terminator
        .clone()
        .or_else(|| server.session_terminator());
    let admission = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_requests));
    let request_timeout = config.request_timeout;
    let ip_rate_limiter = config.ip_rate_limiter.clone();
    let gate_proxies: Arc<[IpNet]> = config.trusted_proxies.clone().into();
    let state = HttpState {
        service: server.service(),
        codec: DefaultCodec::default(),
        origins: config.origins.clone(),
        hosts: config.hosts.clone(),
        sse_keepalive: config.sse_keepalive,
        sse_upgrade_after: config.sse_upgrade_after,
        authenticator: config.authenticator.clone(),
        rate_limiter: config.rate_limiter.clone(),
        session_terminator,
        trusted_proxies: config.trusted_proxies.clone().into(),
        streams: SessionStreams::new(),
        supported_versions: supported_versions.into(),
        shutdown: config.shutdown.clone(),
        stream_budget: StreamBudget::new(config.max_streams, config.max_streams_per_client),
        calls: config.calls.clone(),
    };
    let mut app = Router::new()
        .route(
            &config.path,
            post(mcp_post::<H::Service>)
                .get(mcp_get::<H::Service>)
                .delete(mcp_delete::<H::Service>),
        )
        .layer(DefaultBodyLimit::max(config.max_body_bytes));
    // RFC 9728 Protected Resource Metadata is public (no auth) discovery, at
    // the root and at the location RFC 9728 §3.1 derives for this endpoint
    // (the resource's path inserted after the well-known segment), which is
    // where a correctly configured `resource_metadata` challenge URL points
    // and where MCP clients look first.
    if config.authenticator.is_some() {
        app = app.route(
            RESOURCE_METADATA_PATH,
            axum::routing::get(resource_metadata::<H::Service>),
        );
        let path_inserted = format!("{RESOURCE_METADATA_PATH}{}", config.path);
        if path_inserted != RESOURCE_METADATA_PATH {
            app = app.route(
                &path_inserted,
                axum::routing::get(resource_metadata::<H::Service>),
            );
        }
    }
    if let Some(path) = &config.health_path {
        app = app.route(path, axum::routing::get(health::<H::Service>));
    }
    #[cfg_attr(not(feature = "websocket"), allow(unused_mut))]
    let mut app = app.with_state(state.clone());
    #[cfg(feature = "websocket")]
    if let Some(ws) = &config.websocket {
        app = app.merge(websocket::routes(server, state, &config, ws));
    }
    #[cfg(not(feature = "websocket"))]
    drop((server, state));
    let app = match config.origins.cors_layer() {
        Some(cors) => app.layer(cors),
        None => app,
    };
    app.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let admission = admission.clone();
            let ip_rate_limiter = ip_rate_limiter.clone();
            let gate_proxies = Arc::clone(&gate_proxies);
            async move {
                // The per-IP gate runs first, before the body is read or a
                // token verified: it is what makes a flood cheap to refuse.
                if let Some(limiter) = &ip_rate_limiter {
                    let (parts, body) = request.into_parts();
                    let key = PeerIp::from_parts(&parts)
                        .client_ip(&gate_proxies)
                        .map_or(RateKey::Global, RateKey::Ip);
                    if let Err(retry_after) = limiter.check(&key) {
                        return too_many_requests(retry_after);
                    }
                    let request = axum::extract::Request::from_parts(parts, body);
                    admit(admission, request_timeout, request, next).await
                } else {
                    admit(admission, request_timeout, request, next).await
                }
            }
        },
    ))
}

/// The admission pool: a permit per in-flight request, held until its body
/// has been streamed out, unless the handler hands it back because the
/// response is a long-lived stream with a slot of its own (see
/// [`Admission`]).
async fn admit(
    admission: Arc<tokio::sync::Semaphore>,
    request_timeout: Duration,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Ok(permit) = admission.try_acquire_owned() else {
        return service_unavailable("too many requests in flight on this endpoint");
    };
    let slot = Admission::new(permit);
    request.extensions_mut().insert(slot.clone());
    let response = match tokio::time::timeout(request_timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => return deadline_passed(),
    };
    let (parts, body) = response.into_parts();
    let stream = futures::stream::unfold(
        (body.into_data_stream(), slot),
        |(mut stream, slot)| async move {
            use futures::StreamExt as _;
            stream.next().await.map(|chunk| (chunk, (stream, slot)))
        },
    );
    Response::from_parts(parts, axum::body::Body::from_stream(stream))
}

/// Streamable HTTP on a socket, as a target for `ServerBuilder::serve`:
/// `server.serve(Http::bind(addr).config(HttpConfig::new()))`.
#[derive(Debug)]
pub struct Http {
    listen: Listen,
    config: HttpConfig,
}

#[derive(Debug)]
enum Listen {
    Addr(SocketAddr),
    Listener(tokio::net::TcpListener),
}

impl Http {
    /// Bind `addr` when serving starts.
    #[must_use]
    pub fn bind(addr: SocketAddr) -> Self {
        Self {
            listen: Listen::Addr(addr),
            config: HttpConfig::default(),
        }
    }

    /// Serve on a listener that is already bound (port `0` in tests, a socket
    /// handed over by a supervisor).
    #[must_use]
    pub fn listener(listener: tokio::net::TcpListener) -> Self {
        Self {
            listen: Listen::Listener(listener),
            config: HttpConfig::default(),
        }
    }

    /// Serve by `config`.
    #[must_use]
    pub fn config(mut self, config: HttpConfig) -> Self {
        self.config = config;
        self
    }
}

impl Serve for Http {
    async fn serve<H: ServerHandle>(self, server: H) -> Result<(), ProtocolError> {
        let listener = match self.listen {
            Listen::Addr(addr) => tokio::net::TcpListener::bind(addr)
                .await
                .map_err(HttpError::Io)?,
            Listen::Listener(listener) => listener,
        };
        serve_listener(listener, server, self.config).await?;
        Ok(())
    }
}

/// Serve `server` over Streamable HTTP on `addr` until the configured shutdown
/// token fires (or forever, with the default token). The same as
/// `server.serve(Http::bind(addr).config(config))`.
///
/// # Errors
/// Returns [`HttpError::Io`] if the listener cannot bind or the server loop fails.
pub async fn serve_http<H: ServerHandle>(
    addr: SocketAddr,
    server: H,
    config: HttpConfig,
) -> Result<(), HttpError> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_listener(listener, server, config).await
}

/// When the shutdown token fires, live `subscriptions/listen` requests get
/// their closing responses first; only then does the endpoint stop accepting
/// and start draining, with the listen streams still open to carry them.
async fn serve_listener<H: ServerHandle>(
    listener: tokio::net::TcpListener,
    server: H,
    mut config: HttpConfig,
) -> Result<(), HttpError> {
    let shutdown_timeout = config.shutdown_timeout;
    let (shutdown, closing) =
        close_then_shut_down(&server, config.shutdown.clone(), shutdown_timeout);
    config.shutdown = shutdown.clone();
    let signal = shutdown.clone();
    let calls = config.calls.clone();
    #[cfg(feature = "websocket")]
    let websocket = config.websocket.clone();
    let app = router(server, config);
    if let Ok(addr) = listener.local_addr() {
        tracing::info!(%addr, "turbomcp http transport listening");
    }
    // `with_connect_info` so the rate limiter can key anonymous requests on the
    // peer IP (a no-op when no limiter is configured).
    // Small SSE events shouldn't wait on Nagle's algorithm.
    let listener = listener.tap_io(|tcp| {
        if let Err(e) = tcp.set_nodelay(true) {
            tracing::debug!(error = %e, "could not set TCP_NODELAY");
        }
    });
    let serving = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { shutdown.cancelled().await });
    use std::future::IntoFuture as _;
    let serving = serving.into_future();
    tokio::pin!(serving);
    tokio::pin!(closing);
    tokio::select! {
        result = &mut serving => result?,
        () = signal.cancelled() => {
            if let Ok(result) = tokio::time::timeout(shutdown_timeout, &mut serving).await { result?; }
        }
        () = &mut closing => unreachable!("the close task never completes"),
    }
    // A call whose client went away (a session's call outlives its
    // response) is not one of axum's connections; wait for those too.
    calls.close();
    if tokio::time::timeout(shutdown_timeout, calls.wait())
        .await
        .is_err()
    {
        tracing::warn!(
            running = calls.len(),
            "requests still running at the end of the shutdown deadline"
        );
    }
    // Upgraded sockets outlive the HTTP server's own drain: axum hands them
    // off and stops tracking them.
    #[cfg(feature = "websocket")]
    if let Some(websocket) = websocket {
        websocket.drained(shutdown_timeout).await;
    }
    Ok(())
}

// ---- handlers ----------------------------------------------------------------

async fn mcp_post<S>(
    State(state): State<HttpState<S>>,
    Extension(admission): Extension<Admission>,
    peer: PeerIp,
    headers: HeaderMap,
    body: Bytes,
) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    if let Some(rejection) = check_origin(&state.origins, &headers) {
        return rejection;
    }
    if let Some(rejection) = check_host(&state.hosts, &headers) {
        return rejection;
    }
    // Transports spec §Sending Messages to the Server: the client MUST list
    // both `application/json` and `text/event-stream` in `Accept` — a POST
    // may answer with either.
    if !(accepts(&headers, &mime::APPLICATION_JSON) && accepts(&headers, &mime::TEXT_EVENT_STREAM))
    {
        return not_acceptable_rejection(
            "POST requires an Accept header listing both application/json and text/event-stream",
        );
    }

    // Resource-server auth (if configured), before the body is decoded: an
    // unauthenticated caller shouldn't get to spend a JSON parse. A rejected
    // request never dispatches.
    let authenticated = match enforce_auth(&state, &headers).await {
        Ok(authenticated) => authenticated,
        Err(rejection) => return *rejection,
    };
    let subject = authenticated.as_ref().and_then(|a| a.subject.clone());
    let client_ip = peer.client_ip(&state.trusted_proxies);

    let mut msg = match turbomcp_core::codec::decode_message(&state.codec, &body) {
        Ok(msg) => msg,
        Err(bad) => return invalid_frame_response(&bad),
    };

    // What this endpoint knows about the request travels beside the message,
    // where the client can't write: the verified identity, the session, the
    // mirrors that arrived, and where session streams live.
    let mut ext = Extensions::new().with(state.streams.clone());
    if let Some(authenticated) = authenticated {
        ext.insert(authenticated.identity);
    }

    // Rate limit (if configured) per identity: authenticated → per-subject,
    // anonymous → per source IP. Over budget → 429 + Retry-After, before any
    // dispatch.
    if let Some(rejection) = enforce_rate_limit(&state, subject.as_deref(), client_ip) {
        return rejection;
    }

    // "If the server receives a request with an invalid **or unsupported**
    // `MCP-Protocol-Version`, it MUST respond with `400 Bad Request`."
    //
    // Unsupported is measured against *this server's* set, which
    // `#[server(protocols(…))]` narrows — not against every version the crate
    // can parse. Tolerating a real-but-unserved version (`2024-11-05`, or
    // `2025-11-25` on a `2025-06-18`-only server) meant answering it in a wire
    // shape the client never asked for.
    let header_version = headers
        .get(&headers::PROTOCOL_VERSION)
        .and_then(|v| v.to_str().ok());
    if let Some(v) = header_version
        && !state.serves(v)
    {
        return version_header_rejection(request_id(&msg).as_ref(), v, &state.supported_versions);
    }
    // Header/body mirror validation (draft envelope): version header must
    // match a body-declared version; `Mcp-Method`/`Mcp-Name`/`Mcp-Param-*`
    // must mirror the body on draft requests. Runs BEFORE the dual-stack
    // routing below, which stamps a negotiated version into version-less
    // legacy bodies.
    if let Some(rejection) = validate_request_headers(&msg, &headers) {
        return rejection;
    }
    // Whether this request rides the stateless wire. The body's own `_meta`
    // is the primary signal, but a request whose envelope is *missing* has no
    // body signal to read — the header is what still identifies the wire, and
    // rejecting such a request is the whole point (SEP-2575).
    let modern_wire = declared_version(&msg)
        .as_deref()
        .or(header_version)
        .is_some_and(|v| ProtocolVersion::from_wire(v) == ProtocolVersion::V2026_07_28);
    let stateless_request = modern_wire && matches!(&msg, JsonRpcMessage::Request(_));

    // SEP-2575: a stateless request MUST carry `protocolVersion` and
    // `clientCapabilities` in `_meta`. The dispatcher rejects these too (every
    // transport must), but the status code is the transport's to set: these
    // are malformed requests, not application errors, so they are 400 rather
    // than the usual 200-with-an-error-body.
    if stateless_request
        && let JsonRpcMessage::Request(r) = &msg
        && let Some(field) = meta::missing_request_envelope_field(r.params.as_ref())
    {
        return envelope_rejection(&r.id, field);
    }

    // Hand the dispatcher the `Mcp-Param-*` mirrors that arrived, name
    // (lowercased) to raw value. It knows the tool's schema, so it is the one
    // that can tell which argument each mirrors and check the value; only we
    // know which headers were sent, and an *omitted* mirror is a validation
    // failure (SEP-2243 §Server Validation) the body alone cannot reveal. A
    // value that isn't visible ASCII is passed as `null`, which the dispatcher
    // refuses if the header is one the tool declares.
    if stateless_request {
        ext.insert(ObservedHeaders(
            headers
                .iter()
                .filter_map(|(name, value)| {
                    let param = name.as_str().strip_prefix(headers::MCP_PARAM_PREFIX)?;
                    let value = value.to_str().ok().map(str::to_owned);
                    Some((param.to_ascii_lowercase(), value))
                })
                .collect(),
        ));
    }

    // "An `Mcp-Session-Id` header on a request: ignore it" (2026-07-28, for a
    // stateless request). Honouring it sent a modern request carrying a stale
    // id (a dual-era client, a gateway replaying a sticky header) down the
    // session path to a bodiless 404, which the spec's own fallback algorithm
    // reads as "legacy HTTP+SSE server".
    let session_header = if modern_wire {
        None
    } else {
        headers
            .get(&headers::SESSION_ID)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };

    // Dual-stack routing (module docs): legacy traffic carries its session;
    // modern stateless bodies pass through untouched.
    let is_initialize = msg.method() == Some("initialize");
    if state.authenticator.is_some()
        && state.session_terminator.is_none()
        && (is_initialize || session_header.is_some())
    {
        return sessions_need_an_owner();
    }
    let mut minted_session = None;
    if is_initialize {
        let sid = uuid::Uuid::new_v4().to_string();
        ext.insert(SessionId::new(sid.as_str()));
        minted_session = Some(sid);
    } else if let Some(sid) = session_header {
        if let Some(terminator) = &state.session_terminator {
            match terminator.owns(&sid, subject.as_deref()).await {
                Ok(true) => {}
                Ok(false) => return session_not_found(request_id(&msg).as_ref()),
                Err(e) => return protocol_error_response(&e, request_id(&msg)),
            }
        }
        if !message_has_version(&msg) {
            // What this session actually negotiated, in preference order:
            //
            // 1. The session's stored version. The spec has the server fall
            //    back to assuming a version only when it "has no other way to
            //    identify the version — for example, by relying on the
            //    protocol version negotiated during initialization", and a
            //    live session is precisely that other way. Reading the header
            //    first meant a client that sent something other than what it
            //    negotiated got answered in the shape it typed, not the one
            //    the two ends agreed on.
            // 2. `MCP-Protocol-Version`, which both revisions require on every
            //    post-`initialize` request, for a session this endpoint's
            //    backend cannot look up.
            // 3. `2025-11-25`, which keeps tolerant clients (rmcp, Codex)
            //    working when neither is available.
            let stored = match &state.session_terminator {
                Some(t) => t.negotiated_version(&sid).await,
                None => None,
            };
            let session_version = stored
                .or_else(|| header_version.map(ProtocolVersion::from_wire))
                .filter(ProtocolVersion::is_stateful)
                .unwrap_or(ProtocolVersion::V2025_11_25);
            meta::set_request_meta(
                &mut msg,
                meta::keys::PROTOCOL_VERSION,
                json!(session_version.as_str()),
            );
        }
        ext.insert(SessionId::new(sid));
    } else if !modern_wire && msg.method() != Some("server/discover") {
        // Neither a session nor the stateless envelope: the session path
        // requires a session (spec §Session Management), and 2026-07-28 "MUST
        // reject a request without the header". This used to be dispatched
        // and answered `200` with an in-band error, which a probe or a
        // gateway counts as a working call. `server/discover`, like
        // `initialize`, is how a client finds out which of the two it is
        // talking to, so it needs neither.
        return session_required_rejection(request_id(&msg).as_ref());
    }

    // A modern `subscriptions/listen` request answers with a long-lived SSE
    // stream rather than a JSON body. (A legacy-stamped or malformed listen
    // comes back from the dispatcher as an error *response*, which the SSE
    // path renders as plain JSON — so the divert is safe on method alone.)
    let is_listen = msg.method() == Some("subscriptions/listen");
    let is_request = matches!(&msg, JsonRpcMessage::Request(_));
    let id = request_id(&msg);
    let request = McpRequest {
        message: msg,
        extensions: ext,
    };
    if is_listen {
        let client = client_key(subject.as_deref(), client_ip);
        return listen_sse(&state, request, client, &admission).await;
    }

    // Every other *request* takes the lazy-upgrade path: plain JSON unless the
    // handler emits server→client messages mid-flight. `initialize` stays on
    // the inline path below — its response must carry the minted session
    // header, and the handshake never streams.
    if !is_initialize && is_request {
        return request_post(&state, request, stateless_request, &admission).await;
    }

    let mut svc = state.service.clone();
    if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
        return protocol_error_response(&e, id);
    }
    match catch_handler_panic(id.clone(), svc.call(request)).await {
        Ok(Some(reply)) => {
            let mut resp = encode_json_response(&state.codec, &reply);
            // `initialize` reaches this inline path rather than `request_post`,
            // so it needs the same 404 upgrade: on the stateless wire the
            // method was removed, and the dispatcher answers `-32601`.
            apply_stateless_error_status(&mut resp, &reply, stateless_request);
            // A successful initialize hands the minted session back to the
            // client as the Mcp-Session-Id header.
            if let Some(sid) = minted_session
                && matches!(&reply, JsonRpcMessage::Response(r) if r.error.is_none())
                && let Ok(value) = HeaderValue::from_str(&sid)
            {
                resp.headers_mut().insert(headers::SESSION_ID, value);
            }
            resp
        }
        Ok(None) => StatusCode::ACCEPTED.into_response(), // notification: no body
        Err(e) => protocol_error_response(&e, id),
    }
}

/// Give a stateless-wire reply the HTTP status its JSON-RPC error code calls
/// for (transports spec / SEP-2575). Two codes are not "the request was fine,
/// the operation failed", so they do not get the usual `200`:
///
/// - `-32601` → `404`: on this wire the method does not exist, however well an
///   earlier revision defined it.
/// - `-32021` → `400`: the client did not declare a capability the call needs,
///   so the request was never valid to send.
/// - `-32020` → `400`: header/body validation failed. Most of these are caught
///   in [`validate_request_headers`] before dispatch, but a *missing*
///   `Mcp-Param-*` mirror is only detectable once the tool's schema is known,
///   so that one comes back from the dispatcher and is mapped here.
///
/// Everything else stays `200` with the error in the body — including
/// `-32602`, which a perfectly well-formed request earns by naming a missing
/// resource or a bad tool argument.
fn apply_stateless_error_status(resp: &mut Response, reply: &JsonRpcMessage, enabled: bool) {
    let JsonRpcMessage::Response(r) = reply else {
        return;
    };
    let Some(code) = r.error.as_ref().map(|e| e.code) else {
        return;
    };
    // `UnsupportedProtocolVersionError`: "For HTTP, the response status code
    // MUST be `400 Bad Request`", whichever wire the body arrived on.
    if code == turbomcp_core::codes::UNSUPPORTED_PROTOCOL_VERSION {
        *resp.status_mut() = StatusCode::BAD_REQUEST;
        return;
    }
    if !enabled {
        return;
    }
    if code == turbomcp_core::codes::METHOD_NOT_FOUND {
        *resp.status_mut() = StatusCode::NOT_FOUND;
    } else if code == turbomcp_core::codes::MISSING_REQUIRED_CLIENT_CAPABILITY
        || code == turbomcp_core::codes::HEADER_MISMATCH
    {
        *resp.status_mut() = StatusCode::BAD_REQUEST;
    }
}

/// Open the SSE stream for a `subscriptions/listen` request: register a
/// per-stream writer (under a minted connection id) so the dispatcher's
/// subscription registry can reach this response, dispatch the listen, and —
/// if it was accepted — stream every pushed message as an SSE event.
///
/// The writer registration travels inside the stream state, so a client
/// disconnect (axum drops the body) unregisters it; the registry prunes the
/// subscription at its next publish (transports spec §Cancellation: closing
/// the stream is the cancellation signal).
async fn listen_sse<S>(
    state: &HttpState<S>,
    mut request: McpRequest,
    client: RateKey,
    admission: &Admission,
) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    // The slot is taken before the listen is dispatched, so a refused stream
    // never subscribes.
    let slot = match state.stream_budget.admit(client) {
        Ok(slot) => slot,
        Err(rejection) => return *rejection,
    };
    let (mut outlet, rx) = Outlet::open("http-sse", &mut request);
    outlet._slot = Some(slot);

    let id = request_id(&request.message);
    let mut svc = state.service.clone();
    if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
        return protocol_error_response(&e, id);
    }
    match catch_handler_panic(id.clone(), svc.call(request)).await {
        // Accepted: no JSON-RPC response; the ack notification is already in
        // the channel as the stream's first event.
        Ok(None) => {}
        // Rejected in-band (bad filter, legacy path, unsupported version).
        Ok(Some(reply)) => return encode_json_response(&state.codec, &reply),
        Err(e) => return protocol_error_response(&e, id),
    }

    admission.release();
    sse_response(
        state.codec,
        rx,
        outlet,
        state.sse_keepalive,
        state.shutdown.clone(),
    )
}

/// Dispatch one JSON-RPC request with a per-request server→client channel
/// (transports spec §Sending Messages: the server answers each POSTed request
/// with either a single JSON object or an SSE stream scoped to that request).
///
/// The request carries a [`Peer`] for its own channel under a minted
/// per-request connection id, so anything the handler emits mid-flight —
/// inline bidi requests on the legacy path, progress, log messages — reaches
/// this response. A request that answers within `sse_upgrade_after` and emits
/// nothing gets plain JSON. Otherwise the response becomes
/// `text/event-stream` at the first mid-flight message or when
/// `sse_upgrade_after` passes, whichever is first, carrying the
/// request-related messages followed by the final response, which terminates
/// the stream.
///
/// The call runs on a task of its own, which owns the channel and the
/// request's admission slot. What a client disconnect does to it depends on
/// the wire:
///
/// - `2026-07-28`: "the server MUST treat a client disconnect as cancellation
///   of that request", so the response holds an abort handle and dropping it
///   (axum drops the body) stops the call.
/// - `2025-06-18` / `2025-11-25`: "Disconnection SHOULD NOT be interpreted as
///   the client cancelling its request." A network blip or a proxy's idle cut
///   used to abort a side-effecting tool halfway; the call now runs to its
///   end, and is stopped by `notifications/cancelled`, by its session ending,
///   or by shutdown.
async fn request_post<S>(
    state: &HttpState<S>,
    mut request: McpRequest,
    stateless_request: bool,
    admission: &Admission,
) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    let JsonRpcMessage::Request(req) = &request.message else {
        unreachable!("request_post is only called for requests");
    };
    let request_id = req.id.clone();
    let detach = !stateless_request && request.extensions.get::<SessionId>().is_some();
    // No event `id` on this stream, because this endpoint does not replay.
    //
    // Attaching one is a MAY, and it belongs entirely to §Resumability and
    // Redelivery: an id is what tells a client it may reconnect with
    // `Last-Event-ID` and be caught up. Nothing here reads that header — and
    // v4's own client sends it — so priming the stream turned a visible
    // disconnect into a silent gap the client believed it had recovered from.
    // A stream with no ids is plainly not resumable, which the spec allows and
    // a client can see.
    let (outlet, mut rx) = Outlet::open("http-post", &mut request);

    let mut svc = state.service.clone();
    if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
        return protocol_error_response(&e, Some(request_id));
    }
    let call = catch_handler_panic(Some(request_id.clone()), svc.call(request));
    // The request slot goes with the call, so a call that outlives its
    // response still counts against `max_concurrent_requests`.
    let slot = admission.take();
    // The call's outcome travels apart from its mid-flight messages, so a
    // protocol error keeps its HTTP status (404 for an unknown session, 503
    // for a store outage). The channel closes exactly when the task ends (the
    // outlet holds its only strong sender), after the outcome is sent: drain
    // the channel, then read the outcome, and nothing is out of order.
    let (outcome_tx, outcome) = tokio::sync::oneshot::channel();
    let task = state.calls.spawn(async move {
        let _slot = slot;
        let _ = outcome_tx.send(call.await);
        drop(outlet);
    });
    let abort = (!detach).then(|| AbortOnDrop(task.abort_handle()));

    tokio::select! {
        biased;
        first = rx.recv() => match first {
            Some(first) => request_stream(
                state.codec,
                Some(first),
                rx,
                outcome,
                request_id,
                state.sse_keepalive,
                abort,
            ),
            // The call ended without emitting anything first.
            None => match outcome.await {
                Ok(Ok(Some(reply))) => {
                    let mut resp = encode_json_response(&state.codec, &reply);
                    apply_stateless_error_status(&mut resp, &reply, stateless_request);
                    resp
                }
                // Cancelled before it answered: nothing to say.
                Ok(Ok(None)) | Err(_) => StatusCode::ACCEPTED.into_response(),
                Ok(Err(e)) => protocol_error_response(&e, Some(request_id)),
            },
        },
        // Still working: send the headers now and let keep-alives hold the
        // connection, rather than keep them back until the deadline (ours,
        // or a proxy's) cuts the call off.
        () = tokio::time::sleep(state.sse_upgrade_after) => request_stream(
            state.codec,
            None,
            rx,
            outcome,
            request_id,
            state.sse_keepalive,
            abort,
        ),
    }
}

/// The legacy (`2025-11-25`) server→client SSE stream: `GET` with an
/// `Mcp-Session-Id` opens the session's notification stream (transports spec
/// §Listening for Messages). The stream is registered as the session's in this
/// endpoint's [`SessionStreams`]; a newer GET stream replaces an older one (the
/// spec forbids broadcasting one message across streams).
/// Resumability (`Last-Event-ID`) is not supported.
///
/// The draft never GETs — it subscribes via `subscriptions/listen` over POST —
/// so a session-less GET answers `405`, which the spec permits.
async fn mcp_get<S>(
    State(state): State<HttpState<S>>,
    Extension(admission): Extension<Admission>,
    peer: PeerIp,
    headers: HeaderMap,
) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    if let Some(rejection) = check_origin(&state.origins, &headers) {
        return rejection;
    }
    if let Some(rejection) = check_host(&state.hosts, &headers) {
        return rejection;
    }
    // A server that supports only 2026-07-28 "SHOULD respond as follows:
    // HTTP GET or DELETE to the MCP endpoint: respond with `405 Method Not
    // Allowed`."
    if !state.serves_sessions() {
        return method_not_allowed("this endpoint serves only 2026-07-28, which has no GET stream");
    }
    // Transports spec §Listening for Messages from the Server: the client
    // MUST list `text/event-stream` in `Accept`.
    if !accepts(&headers, &mime::TEXT_EVENT_STREAM) {
        return not_acceptable_rejection("GET requires an Accept header listing text/event-stream");
    }
    if let Some(rejection) = state.reject_version_header(&headers) {
        return rejection;
    }
    // The GET stream is part of the protected resource; require auth too.
    let subject = match enforce_auth(&state, &headers).await {
        Ok(authenticated) => authenticated.and_then(|a| a.subject),
        Err(rejection) => return *rejection,
    };
    let client_ip = peer.client_ip(&state.trusted_proxies);
    if let Some(rejection) = enforce_rate_limit(&state, subject.as_deref(), client_ip) {
        return rejection;
    }
    let Some(sid) = headers
        .get(&headers::SESSION_ID)
        .and_then(|v| v.to_str().ok())
    else {
        return method_not_allowed(
            "no GET stream without Mcp-Session-Id: 2026-07-28 subscribes via subscriptions/listen",
        );
    };

    if state.authenticator.is_some() && state.session_terminator.is_none() {
        return sessions_need_an_owner();
    }
    if let Some(terminator) = &state.session_terminator {
        match terminator.owns(sid, subject.as_deref()).await {
            Ok(true) => {}
            Ok(false) => return session_not_found(None),
            Err(e) => return protocol_error_response(&e, None),
        }
    }
    let slot = match state
        .stream_budget
        .admit(client_key(subject.as_deref(), client_ip))
    {
        Ok(slot) => slot,
        Err(rejection) => return *rejection,
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<JsonRpcMessage>(SSE_CHANNEL_CAPACITY);
    // One stream per session: a newer GET replaces this one in the registry
    // the dispatcher publishes through, and ends it.
    let peer = Peer::new(format!("http-get-{}", uuid::Uuid::new_v4()), &tx);
    let close = state.shutdown.child_token();
    let guard = state.streams.register(sid, peer, close.clone());
    let outlet = Outlet {
        _tx: tx,
        _guard: Some(guard),
        _slot: Some(slot),
    };
    admission.release();
    sse_response(state.codec, rx, outlet, state.sse_keepalive, close)
}

/// Client-initiated session termination (`2025-11-25` spec §Session
/// Management). With a [`SessionTerminator`] configured
/// ([`HttpConfig::with_session_terminator`]): a `DELETE` carrying an
/// `Mcp-Session-Id` ends that session — `204` if it existed, `404` if not.
/// Without one, the spec permits refusing: `405`. The endpoint is part of the
/// protected resource, so the origin + auth guards apply.
async fn mcp_delete<S>(
    State(state): State<HttpState<S>>,
    peer: PeerIp,
    headers: HeaderMap,
) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    if let Some(rejection) = check_origin(&state.origins, &headers) {
        return rejection;
    }
    if let Some(rejection) = check_host(&state.hosts, &headers) {
        return rejection;
    }
    if !state.serves_sessions() {
        return method_not_allowed("this endpoint serves only 2026-07-28, which has no sessions");
    }
    if let Some(rejection) = state.reject_version_header(&headers) {
        return rejection;
    }
    let subject = match enforce_auth(&state, &headers).await {
        Ok(authenticated) => authenticated.and_then(|a| a.subject),
        Err(rejection) => return *rejection,
    };
    // Rate-limit termination like POST/GET — otherwise it's an unthrottled
    // endpoint even though it mutates session state.
    if let Some(rejection) = enforce_rate_limit(
        &state,
        subject.as_deref(),
        peer.client_ip(&state.trusted_proxies),
    ) {
        return rejection;
    }
    let Some(terminator) = &state.session_terminator else {
        return method_not_allowed(
            "client-initiated session termination is not supported; sessions expire by eviction",
        );
    };
    let Some(sid) = headers
        .get(&headers::SESSION_ID)
        .and_then(|v| v.to_str().ok())
    else {
        return session_required_rejection(None);
    };
    match terminator.terminate(sid, subject.as_deref()).await {
        Ok(true) => {
            state.streams.close(sid);
            StatusCode::NO_CONTENT.into_response()
        }
        // Unknown/already-terminated session: the spec maps this to 404 so the
        // client knows it's gone.
        Ok(false) => session_not_found(None),
        Err(e) => protocol_error_response(&e, None),
    }
}

/// The health check ([`HttpConfig::with_health_check`]): `200` while serving,
/// `503` once shutdown has begun.
async fn health<S>(State(state): State<HttpState<S>>) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    if state.shutdown.is_cancelled() {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "draining" })),
        )
            .into_response()
    } else {
        Json(json!({ "status": "ok" })).into_response()
    }
}

/// Serve the RFC 9728 Protected Resource Metadata document (public, no auth).
/// Only routed when an authenticator is configured.
async fn resource_metadata<S>(State(state): State<HttpState<S>>) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    match &state.authenticator {
        Some(authenticator) => Json(authenticator.resource_metadata()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn encode_json_response(codec: &DefaultCodec, msg: &JsonRpcMessage) -> Response {
    match codec.encode(msg) {
        Ok(bytes) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            bytes,
        )
            .into_response(),
        Err(e) => {
            let id = match msg {
                JsonRpcMessage::Response(r) => r.id.clone(),
                _ => None,
            };
            protocol_error_response(&ProtocolError::from(e), id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_error_bridges_into_protocol_error() {
        let err = HttpError::Io(std::io::Error::other("bind failed"));
        match ProtocolError::from(err) {
            ProtocolError::Transport(msg) => assert!(msg.contains("bind failed")),
            other => panic!("expected Transport, got {other:?}"),
        }
    }
}
