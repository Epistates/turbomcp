//! # turbomcp-transport-http
//!
//! The Streamable HTTP transport: a single MCP endpoint served by **axum 0.8**.
//!
//! Unlike stdio (one long-lived bidirectional byte stream driven by the
//! [`serve`](turbomcp_service::serve) loop), HTTP is request/response — axum
//! owns the accept loop and concurrency. So this crate is *not* a
//! [`Transport`](turbomcp_service::Transport) impl; it is a runner that drives
//! an [`McpService`] directly: each `POST` body is decoded to a
//! [`JsonRpcMessage`], handed to a per-request clone of the service, and the
//! reply encoded back.
//!
//! ## Endpoint behavior (dual-stack)
//!
//! - **`POST {path}`** — body is one JSON-RPC message (batches are not supported,
//!   per PLAN §13.1). The client MUST list both `application/json` and
//!   `text/event-stream` in `Accept` (`406` otherwise; RFC 9110 media ranges —
//!   wildcards, `q=` parameters — are honored), and a `GET` MUST list
//!   `text/event-stream`. A notification yields `202 Accepted` with no body. A
//!   request yields either `200 application/json` with the response, or — if
//!   the handler emits server→client messages mid-flight (inline bidi
//!   requests on the legacy path, progress, log messages) — a
//!   `200 text/event-stream` *scoped to that request*: on `2025-11-25`, a
//!   primer event first (an event ID + empty `data`, the spec's SHOULD so the
//!   client always holds a `Last-Event-ID`; the draft dropped resumability, so
//!   its streams are never primed), then the request-related messages as
//!   events, then the final response, which terminates the stream
//!   (transports spec §Sending Messages). A modern `subscriptions/listen`
//!   request yields a long-lived `200 text/event-stream` instead: the
//!   acknowledged notification first, then the opted-in change notifications.
//!   Every SSE response carries keep-alive comments (default 15s) and
//!   `X-Accel-Buffering: no` so proxies don't buffer. Closing a stream is the
//!   cancellation signal for the work it carries.
//! - **`GET {path}`** — with an `Mcp-Session-Id` header: the legacy
//!   (`2025-11-25`) server→client SSE stream for that session (list_changed,
//!   resources/updated). Without one: `405` — the draft replaced the GET
//!   stream with `subscriptions/listen`.
//! - **`DELETE {path}`** — with a [`SessionTerminator`] configured
//!   ([`HttpConfig::with_session_terminator`]): ends the `Mcp-Session-Id`
//!   session (`204`, or `404` if unknown). Without one: `405` — the
//!   `2025-11-25` spec lets a server refuse termination (sessions then expire
//!   by store eviction / idle timeout).
//!
//! ## Dual-stack request routing (PLAN §11)
//!
//! Modern `2026-07-28` requests are stateless (version inside the body's
//! `_meta`) and pass through untouched. The legacy `2025-11-25` stateful path
//! is routed from HTTP headers, asserted toward the dispatcher via internal
//! `_meta` keys (inbound bodies are sanitized first so clients can't forge
//! them):
//!
//! 1. An explicit but unsupported `MCP-Protocol-Version` header → `400`.
//! 2. Body is `initialize` → mint a session id, attach it to the message; on
//!    success the response carries it back as `Mcp-Session-Id`.
//! 3. `Mcp-Session-Id` header present → attach the session id (and, for
//!    version-less bodies, the legacy version) and dispatch; an unknown
//!    session answers `404` so the client re-initializes.
//! 4. `MCP-Protocol-Version: 2025-11-25` without a session id (and not
//!    `initialize`) → `400` (the legacy path requires a session).
//!
//! ## Security
//!
//! - **Origin / DNS-rebinding guard:** a request carrying an `Origin` header that
//!   isn't on the allowlist is rejected with `403`. The default allowlist is
//!   empty, so only `Origin`-less (non-browser) clients pass — the secure default
//!   for a local server. Use [`HttpConfig::allow_origin`] /
//!   [`HttpConfig::allow_any_origin`] to widen it. For defense in depth against
//!   non-browser clients (which can spoof `Host` and send no `Origin`),
//!   [`HttpConfig::allow_host`] pins the server's expected `Host`(s) and rejects
//!   others with `403`.
//! - **Body limit:** `POST` bodies above [`HttpConfig::max_body_bytes`] (default
//!   1 MiB) are rejected with `413`.
//! - **CORS:** off by default; [`HttpConfig::enable_cors`] adds a permissive
//!   `tower-http` `CorsLayer` (intended for `allow_any_origin` dev setups).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::{Future, poll_fn};
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use tower_http::cors::CorsLayer;
use turbomcp_codec::{Codec, DefaultCodec};
use turbomcp_core::{JsonRpcMessage, ProtocolVersion, RequestId, meta};
use turbomcp_service::{
    AuthDecision, CancellationToken, HttpAuthenticator, McpService, ProtocolError, RateKey,
    RateLimiter, SessionTerminator, mcp_headers, outbound,
};

/// The session header of the `2025-11-25` Streamable HTTP transport.
const HEADER_SESSION_ID: HeaderName = HeaderName::from_static("mcp-session-id");
/// The per-request version header of the Streamable HTTP transport.
const HEADER_PROTOCOL_VERSION: HeaderName = HeaderName::from_static("mcp-protocol-version");
/// The draft's `Mcp-Method` standard request header (mirrors `method`).
const HEADER_MCP_METHOD: HeaderName = HeaderName::from_static("mcp-method");
/// The draft's `Mcp-Name` standard request header (mirrors `params.name`/`.uri`).
const HEADER_MCP_NAME: HeaderName = HeaderName::from_static("mcp-name");
/// Header-name prefix carrying an `x-mcp-header` tool parameter
/// (`Mcp-Param-<name>`). Compared case-insensitively (HTTP lowercases header
/// names).
const MCP_PARAM_PREFIX: &str = "mcp-param-";

/// The protocol version a message's own `_meta` declares, if any — the
/// stateless draft envelope. Must be read **before** the dual-stack routing
/// stamps a session's negotiated version into version-less legacy bodies.
fn declared_version(msg: &JsonRpcMessage) -> Option<String> {
    let params = match msg {
        JsonRpcMessage::Request(r) => r.params.as_ref(),
        JsonRpcMessage::Notification(n) => n.params.as_ref(),
        JsonRpcMessage::Response(_) => None,
    }?;
    params
        .get("_meta")?
        .get(meta::keys::PROTOCOL_VERSION)?
        .as_str()
        .map(str::to_owned)
}

/// Validate the transport's request-metadata headers against the body
/// (transports spec §Request Metadata / §Server Validation). Applies to
/// messages whose body `_meta` declares a protocol version — the stateless
/// draft envelope; the legacy `2025-11-25` session flow keeps its
/// negotiated-version tolerance. Any failure is `400` + a
/// `HeaderMismatch` JSON-RPC error (`-32001`).
///
/// Headers are pure **mirrors** — the body stays authoritative and values are
/// never sourced *from* headers (the earlier fill-absent `Mcp-Param-*` merge
/// is gone; it let a header inject an argument the body omitted).
fn validate_request_headers(msg: &JsonRpcMessage, headers: &HeaderMap) -> Option<Response> {
    let declared = declared_version(msg);
    let header_version = headers
        .get(&HEADER_PROTOCOL_VERSION)
        .and_then(|v| v.to_str().ok());

    // The mirror invariant: when both the header and the body name a version,
    // they MUST agree — whatever the versions.
    if let (Some(h), Some(d)) = (header_version, declared.as_deref())
        && h != d
    {
        return Some(header_mismatch_rejection(&format!(
            "MCP-Protocol-Version header ({h}) does not match the request body ({d})"
        )));
    }

    // The remaining rules are the draft transport's: they apply when the body
    // carries the draft's stateless envelope. A body declaring a *legacy*
    // version without a header keeps `2025-11-25`'s softer absence rule (the
    // session flow governs); a *header*-only draft version on an
    // envelope-less body (e.g. a notification, whose params carry no
    // envelope) requires nothing further.
    let declared_draft = declared
        .as_deref()
        .is_some_and(|d| ProtocolVersion::from_wire(d) == ProtocolVersion::Draft);
    if !declared_draft {
        return None;
    }
    if header_version.is_none() {
        // The draft requires the version header on every POST; this server
        // supports no pre-2025-06-18 clients, so absence is a rejection.
        return Some(header_mismatch_rejection(
            "missing required MCP-Protocol-Version header",
        ));
    }
    let JsonRpcMessage::Request(req) = msg else {
        return None;
    };

    // `Mcp-Method` is required on every request POST and mirrors `method`.
    match headers
        .get(&HEADER_MCP_METHOD)
        .and_then(|v| v.to_str().ok())
    {
        None => {
            return Some(header_mismatch_rejection(
                "missing required Mcp-Method header",
            ));
        }
        Some(m) if m != req.method => {
            return Some(header_mismatch_rejection(&format!(
                "Mcp-Method header ({m}) does not match the request body ({})",
                req.method
            )));
        }
        Some(_) => {}
    }

    // `Mcp-Name` is required for `tools/call`/`resources/read`/`prompts/get`
    // and mirrors `params.name`/`params.uri` (Base64 sentinel decoded).
    if let Some(field) = mcp_headers::name_field_for(&req.method) {
        let body_value = req
            .params
            .as_ref()
            .and_then(|p| p.get(field))
            .and_then(serde_json::Value::as_str);
        let Some(raw) = headers.get(&HEADER_MCP_NAME).and_then(|v| v.to_str().ok()) else {
            return Some(header_mismatch_rejection(
                "missing required Mcp-Name header",
            ));
        };
        let Some(decoded) = mcp_headers::decode_value(raw) else {
            return Some(header_mismatch_rejection(
                "malformed Base64 sentinel in Mcp-Name header",
            ));
        };
        if body_value != Some(decoded.as_str()) {
            return Some(header_mismatch_rejection(&format!(
                "Mcp-Name header does not match the request body's `{field}`"
            )));
        }
    }

    // Every `Mcp-Param-{name}` naming a `tools/call` argument must match it
    // after decoding and type rendering. A header matching no argument is
    // unrecognized here (it may use a custom `x-mcp-header` name mapping) and
    // is ignored, per the intermediary forwarding rule.
    if req.method == "tools/call"
        && let Some(args) = req
            .params
            .as_ref()
            .and_then(|p| p.get("arguments"))
            .and_then(serde_json::Value::as_object)
    {
        for (name, value) in headers {
            let Some(param) = name.as_str().strip_prefix(MCP_PARAM_PREFIX) else {
                continue;
            };
            let Some((_, body_value)) = args.iter().find(|(k, _)| k.eq_ignore_ascii_case(param))
            else {
                continue;
            };
            let Ok(raw) = value.to_str() else {
                return Some(header_mismatch_rejection(&format!(
                    "Mcp-Param-{param} header contains invalid characters"
                )));
            };
            let Some(decoded) = mcp_headers::decode_value(raw) else {
                return Some(header_mismatch_rejection(&format!(
                    "malformed Base64 sentinel in Mcp-Param-{param} header"
                )));
            };
            let Some(rendered) = mcp_headers::render_argument(body_value) else {
                return Some(header_mismatch_rejection(&format!(
                    "Mcp-Param-{param} mirrors a non-primitive body argument"
                )));
            };
            if decoded != rendered {
                return Some(header_mismatch_rejection(&format!(
                    "Mcp-Param-{param} header does not match the request body argument"
                )));
            }
        }
    }

    None
}

/// Buffered events per SSE stream; a consumer this far behind backpressures
/// publishers (the registry awaits `send`).
const SSE_CHANNEL_CAPACITY: usize = 256;
/// Default keep-alive comment interval — short enough to outlive common
/// proxy/LB idle timeouts (often 30–60s).
const DEFAULT_SSE_KEEPALIVE: Duration = Duration::from_secs(15);

/// Where a request's `Origin` header is checked against (DNS-rebinding guard).
#[derive(Clone, Debug)]
enum OriginPolicy {
    /// Reject any request whose `Origin` isn't in this list. An empty list lets
    /// only `Origin`-less (non-browser) clients through — the secure default.
    Allowlist(Vec<String>),
    /// Accept any `Origin` (development only).
    Any,
}

/// Where a request's `Host` header is checked against — DNS-rebinding defense in
/// depth, complementing [`OriginPolicy`] for non-browser clients that can spoof
/// `Host` (the `Origin` guard only covers browsers).
#[derive(Clone, Debug)]
enum HostPolicy {
    /// Accept any `Host` (the default — suited to deployments behind a proxy or
    /// load balancer that rewrites `Host`).
    Any,
    /// Reject any request whose `Host` isn't in this list. Lets a server that
    /// knows its expected host(s) refuse a spoofed `Host`.
    Allowlist(Vec<String>),
}

/// The well-known path RFC 9728 Protected Resource Metadata is served at.
const RESOURCE_METADATA_PATH: &str = "/.well-known/oauth-protected-resource";

/// Configuration for the HTTP endpoint. Construct with [`HttpConfig::new`] and
/// chain the builder methods.
#[derive(Clone)]
pub struct HttpConfig {
    path: String,
    max_body_bytes: usize,
    origins: OriginPolicy,
    hosts: HostPolicy,
    cors: bool,
    shutdown: CancellationToken,
    sse_keepalive: Duration,
    authenticator: Option<Arc<dyn HttpAuthenticator>>,
    rate_limiter: Option<Arc<dyn RateLimiter>>,
    session_terminator: Option<Arc<dyn SessionTerminator>>,
    trusted_proxies: Vec<IpAddr>,
}

impl core::fmt::Debug for HttpConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpConfig")
            .field("path", &self.path)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("origins", &self.origins)
            .field("hosts", &self.hosts)
            .field("cors", &self.cors)
            .field("sse_keepalive", &self.sse_keepalive)
            .field("authenticator", &self.authenticator.is_some())
            .field("rate_limiter", &self.rate_limiter.is_some())
            .field("session_terminator", &self.session_terminator.is_some())
            .field("trusted_proxies", &self.trusted_proxies)
            .finish()
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            path: "/mcp".to_owned(),
            max_body_bytes: 1 << 20, // 1 MiB
            origins: OriginPolicy::Allowlist(Vec::new()),
            hosts: HostPolicy::Any,
            cors: false,
            shutdown: CancellationToken::new(),
            sse_keepalive: DEFAULT_SSE_KEEPALIVE,
            authenticator: None,
            rate_limiter: None,
            session_terminator: None,
            trusted_proxies: Vec::new(),
        }
    }
}

impl HttpConfig {
    /// Default configuration: `POST /mcp`, 1 MiB body limit, Origin-less only.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the endpoint path (default `/mcp`).
    #[must_use]
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Set the maximum accepted request-body size in bytes (default 1 MiB).
    #[must_use]
    pub fn max_body_bytes(mut self, bytes: usize) -> Self {
        self.max_body_bytes = bytes;
        self
    }

    /// Add an allowed `Origin` (exact match, e.g. `https://app.example.com`).
    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        match &mut self.origins {
            OriginPolicy::Allowlist(list) => list.push(origin.into()),
            OriginPolicy::Any => {}
        }
        self
    }

    /// Accept requests from any `Origin` (development only). Also enables a
    /// permissive CORS layer so browsers can actually use it.
    #[must_use]
    pub fn allow_any_origin(mut self) -> Self {
        self.origins = OriginPolicy::Any;
        self.cors = true;
        self
    }

    /// Add an allowed `Host` (exact match, e.g. `localhost:8080` or
    /// `mcp.example.com`). By default any `Host` is accepted; once at least one
    /// host is allow-listed, a request whose `Host` isn't listed is rejected
    /// with `403`. Combined with [`allow_origin`](Self::allow_origin) this
    /// hardens the server against DNS-rebinding (a spoofed `Host`/`Origin` from
    /// a non-browser client is refused). Leave unset behind a trusted proxy that
    /// rewrites `Host`.
    #[must_use]
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        match &mut self.hosts {
            HostPolicy::Allowlist(list) => list.push(host.into()),
            HostPolicy::Any => self.hosts = HostPolicy::Allowlist(vec![host.into()]),
        }
        self
    }

    /// Toggle the permissive CORS layer (off by default).
    #[must_use]
    pub fn enable_cors(mut self, enabled: bool) -> Self {
        self.cors = enabled;
        self
    }

    /// Provide a cancellation token; firing it triggers axum's graceful shutdown.
    #[must_use]
    pub fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = shutdown;
        self
    }

    /// A clone of the configured shutdown token (a fresh, never-fired token by
    /// default). Lets callers coordinate their own teardown — e.g. `run_http`
    /// gracefully closes `subscriptions/listen` subscriptions when it fires.
    #[must_use]
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Set the SSE keep-alive comment interval (default 15s). Keep it shorter
    /// than the idle timeout of any proxy in front of the server.
    #[must_use]
    pub fn sse_keepalive(mut self, interval: Duration) -> Self {
        self.sse_keepalive = interval;
        self
    }

    /// Protect the endpoint as an OAuth 2.1 resource server: every `POST`/`GET`
    /// must carry a valid `Authorization: Bearer` token (validated by
    /// `authenticator`, e.g. `turbomcp_auth::ResourceServer`), and the RFC
    /// 9728 metadata document is served at
    /// `/.well-known/oauth-protected-resource`. Unauthenticated requests get
    /// the `401`/`403` + `WWW-Authenticate` challenges. stdio is unaffected
    /// (the MCP spec has no stdio auth).
    #[must_use]
    pub fn with_authenticator(mut self, authenticator: Arc<dyn HttpAuthenticator>) -> Self {
        self.authenticator = Some(authenticator);
        self
    }

    /// Rate-limit the endpoint. Each request is charged against an
    /// identity-derived [`RateKey`] — per authenticated subject when the
    /// request carries a valid bearer token, otherwise per source IP — and an
    /// over-budget request gets `429 Too Many Requests` + `Retry-After` before
    /// it ever reaches a handler. Pair with
    /// [`GovernorRateLimiter`](turbomcp_service::GovernorRateLimiter) for the
    /// in-process default. stdio is never rate-limited (single trusted local
    /// connection).
    #[must_use]
    pub fn with_rate_limiter(mut self, rate_limiter: Arc<dyn RateLimiter>) -> Self {
        self.rate_limiter = Some(rate_limiter);
        self
    }

    /// Trust these proxy IPs to set `X-Forwarded-For`. When the direct socket
    /// peer is one of them, the client IP used for rate limiting is taken from
    /// the right of the `X-Forwarded-For` chain (the first hop not itself
    /// trusted) instead of the proxy's own address. Spoofable if you list an
    /// address that isn't actually your proxy — list only your real front ends.
    /// Empty (default) means the raw socket peer is always used.
    #[must_use]
    pub fn with_trusted_proxies(mut self, proxies: impl IntoIterator<Item = IpAddr>) -> Self {
        self.trusted_proxies = proxies.into_iter().collect();
        self
    }

    /// Honor client-initiated session termination: a `DELETE` carrying an
    /// `Mcp-Session-Id` ends that `2025-11-25` session (dropping its state and
    /// subscription routes) and answers `204`; an unknown session answers
    /// `404`. Obtain the terminator from
    /// `VersionDispatcher::session_terminator`. Without it, `DELETE` answers
    /// `405` (the spec permits a server refusing termination).
    #[must_use]
    pub fn with_session_terminator(mut self, terminator: Arc<dyn SessionTerminator>) -> Self {
        self.session_terminator = Some(terminator);
        self
    }
}

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
    authenticator: Option<Arc<dyn HttpAuthenticator>>,
    rate_limiter: Option<Arc<dyn RateLimiter>>,
    session_terminator: Option<Arc<dyn SessionTerminator>>,
    trusted_proxies: Arc<[IpAddr]>,
    /// The configured shutdown token; dedicated `subscriptions/listen` SSE
    /// streams end when it fires (the RC's server-side subscription close).
    shutdown: CancellationToken,
}

/// Build the configured axum [`Router`] for `service` without binding a socket —
/// the unit of composition (mount it under a larger app) and the seam tests
/// drive via `tower::ServiceExt::oneshot`.
pub fn router<S>(service: S, config: HttpConfig) -> Router
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    let state = HttpState {
        service,
        codec: DefaultCodec::default(),
        origins: config.origins.clone(),
        hosts: config.hosts.clone(),
        sse_keepalive: config.sse_keepalive,
        authenticator: config.authenticator.clone(),
        rate_limiter: config.rate_limiter.clone(),
        session_terminator: config.session_terminator.clone(),
        trusted_proxies: config.trusted_proxies.clone().into(),
        shutdown: config.shutdown.clone(),
    };
    let mut app = Router::new()
        .route(
            &config.path,
            post(mcp_post::<S>)
                .get(mcp_get::<S>)
                .delete(mcp_delete::<S>),
        )
        .layer(DefaultBodyLimit::max(config.max_body_bytes));
    // RFC 9728 Protected Resource Metadata is public (no auth) discovery.
    if config.authenticator.is_some() {
        app = app.route(
            RESOURCE_METADATA_PATH,
            axum::routing::get(resource_metadata::<S>),
        );
    }
    let app = if config.cors {
        app.layer(CorsLayer::permissive())
    } else {
        app
    };
    app.with_state(state)
}

/// Serve `service` over Streamable HTTP on `addr` until the configured shutdown
/// token fires (or forever, with the default token).
///
/// # Errors
/// Returns [`HttpError::Io`] if the listener cannot bind or the server loop fails.
pub async fn serve_http<S>(
    addr: SocketAddr,
    service: S,
    config: HttpConfig,
) -> Result<(), HttpError>
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    let shutdown = config.shutdown.clone();
    let app = router(service, config);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "turbomcp http transport listening");
    // `with_connect_info` so the rate limiter can key anonymous requests on the
    // peer IP (a no-op when no limiter is configured).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { shutdown.cancelled().await })
    .await?;
    Ok(())
}

// ---- handlers ----------------------------------------------------------------

async fn mcp_post<S>(
    State(state): State<HttpState<S>>,
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

    let mut msg: JsonRpcMessage = match state.codec.decode(&body) {
        Ok(msg) => msg,
        Err(e) => return parse_error_response(&e.to_string()),
    };

    // Internal `_meta` is transport-owned: strip anything the client forged
    // before asserting our own (see `turbomcp_core::meta::internal`).
    meta::sanitize_inbound(&mut msg);

    // Resource-server auth (if configured): validate the bearer token and
    // inject the principal into internal `_meta` — after sanitize, so a
    // forged identity can't survive. A rejected request never dispatches.
    let subject = match enforce_auth(&state, &headers, Some(&mut msg)).await {
        Ok(subject) => subject,
        Err(rejection) => return rejection,
    };

    // Rate limit (if configured) per identity: authenticated → per-subject,
    // anonymous → per source IP. Over budget → 429 + Retry-After, before any
    // dispatch.
    if let Some(rejection) = enforce_rate_limit(
        &state,
        subject.as_deref(),
        peer.client_ip(&state.trusted_proxies),
    ) {
        return rejection;
    }

    // Transport spec: a `MCP-Protocol-Version` header naming an *unrecognized*
    // version is 400. A recognized-but-older published version (e.g. an
    // established client that keeps sending `2025-03-26`) is tolerated: a
    // stateful session's negotiated version governs dispatch, so the transport
    // should not reject a request bearing a real protocol version.
    let header_version = headers
        .get(&HEADER_PROTOCOL_VERSION)
        .and_then(|v| v.to_str().ok());
    if let Some(v) = header_version
        && !ProtocolVersion::from_wire(v).is_recognized()
    {
        return version_header_rejection(v);
    }
    // Header/body mirror validation (draft envelope): version header must
    // match a body-declared version; `Mcp-Method`/`Mcp-Name`/`Mcp-Param-*`
    // must mirror the body on draft requests. Runs BEFORE the dual-stack
    // routing below, which stamps a negotiated version into version-less
    // legacy bodies.
    if let Some(rejection) = validate_request_headers(&msg, &headers) {
        return rejection;
    }
    // Draft transport: an unimplemented RPC method answers HTTP 404 (with the
    // JSON-RPC `-32601` body) rather than 200.
    let method_not_found_404 = declared_version(&msg)
        .is_some_and(|v| ProtocolVersion::from_wire(&v) == ProtocolVersion::Draft)
        && matches!(&msg, JsonRpcMessage::Request(_));

    let session_header = headers
        .get(&HEADER_SESSION_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Dual-stack routing (module docs): mark legacy traffic via internal meta;
    // modern stateless bodies pass through untouched.
    let is_initialize = msg.method() == Some("initialize");
    let mut minted_session = None;
    if is_initialize {
        let sid = uuid::Uuid::new_v4().to_string();
        meta::set_request_meta(&mut msg, meta::internal::SESSION_ID, json!(sid));
        minted_session = Some(sid);
    } else if let Some(sid) = session_header {
        if !message_has_version(&msg) {
            meta::set_request_meta(
                &mut msg,
                meta::keys::PROTOCOL_VERSION,
                json!(ProtocolVersion::V2025_11_25.as_str()),
            );
        }
        meta::set_request_meta(&mut msg, meta::internal::SESSION_ID, json!(sid));
    } else if header_version == Some(ProtocolVersion::V2025_11_25.as_str()) {
        // Declared-legacy request with no session and not initialize: the
        // stateful path requires a session (spec §Session Management).
        return session_required_rejection();
    }

    // A modern `subscriptions/listen` request answers with a long-lived SSE
    // stream rather than a JSON body. (A legacy-stamped or malformed listen
    // comes back from the dispatcher as an error *response*, which the SSE
    // path renders as plain JSON — so the divert is safe on method alone.)
    if msg.method() == Some("subscriptions/listen") {
        return listen_sse(&state, msg).await;
    }

    // Every other *request* takes the lazy-upgrade path: plain JSON unless the
    // handler emits server→client messages mid-flight. `initialize` stays on
    // the inline path below — its response must carry the minted session
    // header, and the handshake never streams.
    if !is_initialize && matches!(&msg, JsonRpcMessage::Request(_)) {
        return request_post(&state, msg, method_not_found_404).await;
    }

    let mut svc = state.service.clone();
    if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
        return protocol_error_response(&e);
    }
    match svc.call(msg).await {
        Ok(Some(reply)) => {
            let mut resp = encode_json_response(&state.codec, &reply);
            // A successful initialize hands the minted session back to the
            // client as the Mcp-Session-Id header.
            if let Some(sid) = minted_session
                && matches!(&reply, JsonRpcMessage::Response(r) if r.error.is_none())
                && let Ok(value) = HeaderValue::from_str(&sid)
            {
                resp.headers_mut().insert(HEADER_SESSION_ID, value);
            }
            resp
        }
        Ok(None) => StatusCode::ACCEPTED.into_response(), // notification: no body
        Err(e) => protocol_error_response(&e),
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
async fn listen_sse<S>(state: &HttpState<S>, mut msg: JsonRpcMessage) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    let connection_id = format!("http-sse-{}", uuid::Uuid::new_v4());
    let (tx, rx) = tokio::sync::mpsc::channel::<JsonRpcMessage>(SSE_CHANNEL_CAPACITY);
    let registration = outbound::register(&connection_id, tx);
    meta::set_request_meta(
        &mut msg,
        meta::internal::CONNECTION_ID,
        json!(connection_id),
    );

    let mut svc = state.service.clone();
    if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
        return protocol_error_response(&e);
    }
    match svc.call(msg).await {
        // Accepted: no JSON-RPC response; the ack notification is already in
        // the channel as the stream's first event.
        Ok(None) => {}
        // Rejected in-band (bad filter, legacy path, unsupported version).
        Ok(Some(reply)) => return encode_json_response(&state.codec, &reply),
        Err(e) => return protocol_error_response(&e),
    }

    sse_response(
        state.codec,
        rx,
        registration,
        state.sse_keepalive,
        Some(state.shutdown.clone()),
    )
}

/// Dispatch one JSON-RPC request with a per-request server→client channel
/// (transports spec §Sending Messages: the server answers each POSTed request
/// with either a single JSON object or an SSE stream scoped to that request).
///
/// The channel is registered in [`outbound`] under a minted per-request
/// connection id before dispatch, so anything the handler emits mid-flight —
/// inline bidi requests on the legacy path, progress, log messages — reaches
/// this response. If nothing is emitted the reply stays plain JSON; the first
/// mid-flight message upgrades the response to `text/event-stream`, carrying
/// the request-related messages followed by the final response, which
/// terminates the stream. A client disconnect drops the in-flight call —
/// HTTP's cancellation signal.
async fn request_post<S>(
    state: &HttpState<S>,
    mut msg: JsonRpcMessage,
    method_not_found_404: bool,
) -> Response
where
    S: McpService + Clone + Sync,
    S::Future: Send + 'static,
{
    let JsonRpcMessage::Request(req) = &msg else {
        unreachable!("request_post is only called for requests");
    };
    let request_id = req.id.clone();
    let connection_id = format!("http-post-{}", uuid::Uuid::new_v4());
    // If this request upgrades to SSE, `2025-11-25` SHOULD-requires priming
    // the stream with an event carrying an event ID and an empty `data` field
    // so the client always holds a `Last-Event-ID` (transports spec §Sending
    // Messages to the Server). The id is `{connection_id}-0`: globally unique
    // and stream-identifying, as §Resumability requires of event IDs. The
    // draft dropped resumability ("Resumable SSE streams via `Last-Event-ID`
    // are not supported"), so draft streams are never primed.
    let primer = declared_version(&msg)
        .is_none_or(|v| ProtocolVersion::from_wire(&v) != ProtocolVersion::Draft)
        .then(|| format!("{connection_id}-0"));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<JsonRpcMessage>(SSE_CHANNEL_CAPACITY);
    let registration = outbound::register(&connection_id, tx);
    meta::set_request_meta(
        &mut msg,
        meta::internal::CONNECTION_ID,
        json!(connection_id),
    );

    let mut svc = state.service.clone();
    if let Err(e) = poll_fn(|cx| svc.poll_ready(cx)).await {
        return protocol_error_response(&e);
    }
    let mut call = Box::pin(svc.call(msg));

    tokio::select! {
        biased;
        result = call.as_mut() => {
            // Completed without upgrading — but messages may have raced into
            // the channel just before completion; don't lose them.
            let mut events = drain(&mut rx);
            drop(registration);
            match result {
                Ok(Some(reply)) if events.is_empty() => {
                    let mut resp = encode_json_response(&state.codec, &reply);
                    // Draft transport: an unimplemented RPC method is HTTP 404
                    // with the JSON-RPC `-32601` body.
                    if method_not_found_404
                        && let JsonRpcMessage::Response(r) = &reply
                        && r.error.as_ref().is_some_and(|e| e.code == -32601)
                    {
                        *resp.status_mut() = StatusCode::NOT_FOUND;
                    }
                    resp
                }
                Ok(Some(reply)) => {
                    events.push_back(reply);
                    finished_sse(state.codec, primer, events)
                }
                // A request always gets a response from the dispatcher; these
                // arms are defensive.
                Ok(None) if events.is_empty() => StatusCode::ACCEPTED.into_response(),
                Ok(None) => finished_sse(state.codec, primer, events),
                Err(e) => protocol_error_response(&e),
            }
        }
        first = rx.recv() => {
            let Some(first) = first else {
                // Unreachable while `registration` holds the sender.
                return protocol_error_response(&ProtocolError::Internal(
                    "per-request channel closed while registered".to_owned(),
                ));
            };
            streaming_post_sse(
                state.codec,
                primer,
                first,
                PostStream::Run {
                    rx,
                    call,
                    id: request_id,
                    registration,
                },
                state.sse_keepalive,
            )
        }
    }
}

/// State for an upgraded per-request SSE response: keep streaming channel
/// messages while driving the in-flight call; when the call completes, append
/// its final response and end the stream.
enum PostStream<F> {
    /// The request is still in flight.
    Run {
        rx: tokio::sync::mpsc::Receiver<JsonRpcMessage>,
        call: Pin<Box<F>>,
        id: RequestId,
        registration: outbound::WriterGuard,
    },
    /// The call finished; flush the remaining events and close.
    Tail(VecDeque<JsonRpcMessage>),
}

/// The upgraded per-request SSE response (see [`request_post`]). The final
/// response (or a JSON-RPC error built from a [`ProtocolError`]) is the last
/// event; dropping the response body drops the call future.
fn streaming_post_sse<F>(
    codec: DefaultCodec,
    primer: Option<String>,
    first: JsonRpcMessage,
    run: PostStream<F>,
    keepalive: Duration,
) -> Response
where
    F: Future<Output = Result<Option<JsonRpcMessage>, ProtocolError>> + Send + 'static,
{
    let head = futures::stream::iter(
        primer
            .as_deref()
            .map(primer_event)
            .into_iter()
            .chain([sse_event(&codec, &first)])
            .map(Ok::<_, Infallible>),
    );
    let tail = futures::stream::unfold(run, move |state| async move {
        match state {
            PostStream::Run {
                mut rx,
                mut call,
                id,
                registration,
            } => {
                tokio::select! {
                    result = call.as_mut() => {
                        let mut events = drain(&mut rx);
                        drop(registration);
                        match result {
                            Ok(Some(reply)) => events.push_back(reply),
                            Ok(None) => {}
                            Err(e) => events.push_back(e.into_response(id).into()),
                        }
                        let msg = events.pop_front()?;
                        Some((
                            Ok::<_, Infallible>(sse_event(&codec, &msg)),
                            PostStream::Tail(events),
                        ))
                    }
                    msg = rx.recv() => {
                        let msg = msg.expect("sender held by registration");
                        Some((
                            Ok::<_, Infallible>(sse_event(&codec, &msg)),
                            PostStream::Run { rx, call, id, registration },
                        ))
                    }
                }
            }
            PostStream::Tail(mut events) => {
                let msg = events.pop_front()?;
                Some((
                    Ok::<_, Infallible>(sse_event(&codec, &msg)),
                    PostStream::Tail(events),
                ))
            }
        }
    });
    let stream = futures::StreamExt::chain(head, tail);
    let sse = Sse::new(stream).keep_alive(KeepAlive::new().interval(keepalive).text("keep-alive"));
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        sse,
    )
        .into_response()
}

/// A short, complete SSE response for a request that finished before the
/// upgrade decision but raced messages into its channel: the messages, the
/// final response, end of stream.
fn finished_sse(
    codec: DefaultCodec,
    primer: Option<String>,
    events: VecDeque<JsonRpcMessage>,
) -> Response {
    let stream = futures::stream::iter(
        primer
            .as_deref()
            .map(primer_event)
            .into_iter()
            .chain(events.into_iter().map(move |msg| sse_event(&codec, &msg)))
            .map(Ok::<_, Infallible>),
    );
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(stream),
    )
        .into_response()
}

/// Empty the channel without awaiting (post-completion stragglers).
fn drain(rx: &mut tokio::sync::mpsc::Receiver<JsonRpcMessage>) -> VecDeque<JsonRpcMessage> {
    let mut events = VecDeque::new();
    while let Ok(msg) = rx.try_recv() {
        events.push_back(msg);
    }
    events
}

/// The `2025-11-25` stream primer (transports spec §Sending Messages: the
/// server SHOULD immediately send an event ID + empty `data` field so the
/// client always holds a `Last-Event-ID`). axum elides the empty `data:`
/// line, so the wire shape is an `id:`-only block — per the WHATWG SSE
/// processing model that's equivalent: the `id` field sets the client's last
/// event ID the moment it's parsed, and an empty data buffer never dispatches
/// a message event anyway. The id-only shape also sidesteps the empty-`data:`
/// misparse bugs in older SSE clients that v3 needed a comment workaround for.
fn primer_event(id: &str) -> Event {
    Event::default().id(id).data("")
}

/// Encode one message as one `data:` event; an encode failure becomes a
/// comment so the stream survives.
fn sse_event(codec: &DefaultCodec, msg: &JsonRpcMessage) -> Event {
    match codec.encode(msg) {
        Ok(bytes) => Event::default().data(String::from_utf8_lossy(&bytes)),
        Err(e) => {
            tracing::warn!(error = %e, "failed to encode SSE event; skipped");
            Event::default().comment("event encoding failed; skipped")
        }
    }
}

/// The common SSE response shape for the two long-lived stream kinds (modern
/// listen, legacy GET): every channel message becomes one `data:` event;
/// keep-alive comments flow in between; the writer registration travels inside
/// the stream state so dropping the response body unregisters it.
fn sse_response(
    codec: DefaultCodec,
    rx: tokio::sync::mpsc::Receiver<JsonRpcMessage>,
    registration: outbound::WriterGuard,
    keepalive: Duration,
    shutdown: Option<CancellationToken>,
) -> Response {
    let stream = futures::stream::unfold(
        (Some((rx, registration)), codec, shutdown),
        |(live, codec, shutdown)| async move {
            let (mut rx, registration) = live?;
            // A listen stream carries the shutdown token: the subscriptions
            // spec ends a subscription by closing the stream (no closing
            // response), so graceful teardown ends it here. Other streams
            // (per-POST, legacy GET) end on their final response instead.
            let msg = match &shutdown {
                Some(token) => tokio::select! {
                    () = token.cancelled() => None,
                    m = rx.recv() => m,
                }?,
                None => rx.recv().await?,
            };
            let event = sse_event(&codec, &msg);
            // A JSON-RPC *response* ends the stream — the per-POST stream
            // contract ("the final response ends the stream").
            let next = if matches!(msg, JsonRpcMessage::Response(_)) {
                None
            } else {
                Some((rx, registration))
            };
            Some((Ok::<_, Infallible>(event), (next, codec, shutdown)))
        },
    );

    let sse = Sse::new(stream).keep_alive(KeepAlive::new().interval(keepalive).text("keep-alive"));
    // X-Accel-Buffering tells reverse proxies (nginx) not to buffer the
    // stream (transports spec: SHOULD include it on SSE responses).
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        sse,
    )
        .into_response()
}

/// The legacy (`2025-11-25`) server→client SSE stream: `GET` with an
/// `Mcp-Session-Id` opens the session's notification stream (transports spec
/// §Listening for Messages). The stream's writer is registered under the
/// session's [`outbound::session_stream_id`]; a newer GET stream replaces an
/// older one (the spec forbids broadcasting one message across streams).
/// Resumability (`Last-Event-ID`) is not supported.
///
/// The draft never GETs — it subscribes via `subscriptions/listen` over POST —
/// so a session-less GET answers `405`, which the spec permits.
async fn mcp_get<S>(State(state): State<HttpState<S>>, peer: PeerIp, headers: HeaderMap) -> Response
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
    // Transports spec §Listening for Messages from the Server: the client
    // MUST list `text/event-stream` in `Accept`.
    if !accepts(&headers, &mime::TEXT_EVENT_STREAM) {
        return not_acceptable_rejection("GET requires an Accept header listing text/event-stream");
    }
    // The GET stream is part of the protected resource; require auth too.
    let subject = match enforce_auth(&state, &headers, None).await {
        Ok(subject) => subject,
        Err(rejection) => return rejection,
    };
    if let Some(rejection) = enforce_rate_limit(
        &state,
        subject.as_deref(),
        peer.client_ip(&state.trusted_proxies),
    ) {
        return rejection;
    }
    let Some(sid) = headers
        .get(&HEADER_SESSION_ID)
        .and_then(|v| v.to_str().ok())
    else {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "POST")],
            "no GET stream without Mcp-Session-Id: the draft subscribes via subscriptions/listen",
        )
            .into_response();
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<JsonRpcMessage>(SSE_CHANNEL_CAPACITY);
    let registration = outbound::register(outbound::session_stream_id(sid), tx);
    sse_response(state.codec, rx, registration, state.sse_keepalive, None)
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
    let subject = match enforce_auth(&state, &headers, None).await {
        Ok(subject) => subject,
        Err(rejection) => return rejection,
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
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "POST")],
            "client-initiated session termination is not supported; sessions expire by eviction",
        )
            .into_response();
    };
    let Some(sid) = headers
        .get(&HEADER_SESSION_ID)
        .and_then(|v| v.to_str().ok())
    else {
        return (
            StatusCode::BAD_REQUEST,
            "DELETE requires an Mcp-Session-Id header",
        )
            .into_response();
    };
    if terminator.terminate(sid).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        // Unknown/already-terminated session: the spec maps this to 404 so the
        // client knows it's gone.
        StatusCode::NOT_FOUND.into_response()
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

// ---- auth --------------------------------------------------------------------

/// Enforce resource-server auth when configured. `Err(challenge)` rejects the
/// request (401/403 + `WWW-Authenticate`). `Ok(subject)` allows it, yielding the
/// authenticated subject (`None` = anonymous, i.e. no authenticator configured)
/// and — when a message is given — injecting the validated principal into its
/// internal `_meta` so the dispatcher lifts it into the request's identity. A
/// `None` authenticator is an open endpoint (allow, anonymous).
async fn enforce_auth<S>(
    state: &HttpState<S>,
    headers: &HeaderMap,
    msg: Option<&mut JsonRpcMessage>,
) -> Result<Option<String>, Response> {
    let Some(authenticator) = state.authenticator.as_ref() else {
        return Ok(None);
    };
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match authenticator.authenticate(authorization).await {
        AuthDecision::Allow(principal) => {
            let subject = principal
                .get("sub")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            if let Some(msg) = msg {
                meta::set_request_meta(msg, meta::internal::IDENTITY, principal);
            }
            Ok(subject)
        }
        AuthDecision::Challenge {
            status,
            www_authenticate,
        } => Err(challenge_response(status, &www_authenticate)),
    }
}

/// Build an auth-challenge response: the status (401/403) plus the
/// `WWW-Authenticate` header.
fn challenge_response(status: u16, www_authenticate: &str) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::UNAUTHORIZED);
    let header = HeaderValue::from_str(www_authenticate)
        .unwrap_or_else(|_| HeaderValue::from_static("Bearer"));
    (status, [(axum::http::header::WWW_AUTHENTICATE, header)]).into_response()
}

// ---- rate limiting -----------------------------------------------------------

/// The request's peer IP, if axum captured one. An infallible extractor: a
/// real socket carries `ConnectInfo<SocketAddr>` in the request extensions
/// (set by `into_make_service_with_connect_info`); oneshot/test harnesses and
/// mounts without connect info simply have none. `Option<ConnectInfo<_>>` can't
/// be used directly — axum 0.8's `Option` extractor needs
/// `OptionalFromRequestParts`, which `ConnectInfo` doesn't implement.
struct PeerIp {
    socket: Option<IpAddr>,
    forwarded: Option<String>,
}

impl PeerIp {
    /// The effective client IP for rate limiting. If the direct socket peer is a
    /// trusted proxy, walk `X-Forwarded-For` from the right to the first hop that
    /// isn't itself trusted; otherwise use the socket peer as-is.
    fn client_ip(&self, trusted: &[IpAddr]) -> Option<IpAddr> {
        let socket = self.socket?;
        if trusted.is_empty() || !trusted.contains(&socket) {
            return Some(socket);
        }
        let hops: Vec<IpAddr> = self
            .forwarded
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        hops.iter()
            .rev()
            .find(|ip| !trusted.contains(ip))
            .copied()
            .or_else(|| hops.first().copied())
            .or(Some(socket))
    }
}

impl<St: Send + Sync> FromRequestParts<St> for PeerIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &St) -> Result<Self, Infallible> {
        Ok(PeerIp {
            socket: parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| addr.ip()),
            forwarded: parts
                .headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
        })
    }
}

/// Enforce the rate limit when configured. Charges the request against an
/// identity-derived [`RateKey`] — per authenticated `subject`, else per source
/// IP, else a single global bucket — and returns `Some(429)` if over budget.
fn enforce_rate_limit<S>(
    state: &HttpState<S>,
    subject: Option<&str>,
    peer_ip: Option<IpAddr>,
) -> Option<Response> {
    let limiter = state.rate_limiter.as_ref()?;
    let key = match subject {
        Some(sub) => RateKey::Subject(sub.to_owned()),
        None => peer_ip.map_or(RateKey::Global, RateKey::Ip),
    };
    match limiter.check(&key) {
        Ok(()) => None,
        Err(retry_after) => Some(too_many_requests(retry_after)),
    }
}

/// `429 Too Many Requests` with a `Retry-After` header (seconds, rounded up).
fn too_many_requests(retry_after: Duration) -> Response {
    // Round up to whole seconds; a sub-second wait still asks for at least 1s.
    let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
    let secs = secs.max(1);
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": -32000, "message": "rate limit exceeded" },
    });
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, secs.to_string())],
        Json(body),
    )
        .into_response()
}

// ---- helpers -----------------------------------------------------------------

/// Whether the message's `params._meta` already states a protocol version (a
/// modern stateless request does; a legacy post-initialize request doesn't).
fn message_has_version(msg: &JsonRpcMessage) -> bool {
    let params = match msg {
        JsonRpcMessage::Request(r) => r.params.as_ref(),
        JsonRpcMessage::Notification(n) => n.params.as_ref(),
        JsonRpcMessage::Response(_) => None,
    };
    params
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(meta::keys::PROTOCOL_VERSION))
        .is_some()
}

/// `400` + a `HeaderMismatch` JSON-RPC error (`-32001`): an HTTP header did
/// not match the corresponding request-body value, or a required header is
/// missing or malformed (transports spec §Server Validation).
fn header_mismatch_rejection(detail: &str) -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32001,
            "message": format!("header mismatch: {detail}"),
        },
    });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// `400` for an explicit but unsupported `MCP-Protocol-Version` header
/// (`UnsupportedProtocolVersionError`, `-32004`, with the spec-required
/// `data: { supported, requested }`).
fn version_header_rejection(requested: &str) -> Response {
    let supported: Vec<&str> = ProtocolVersion::SUPPORTED
        .iter()
        .map(ProtocolVersion::as_str)
        .collect();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32004,
            "message": format!("unsupported MCP-Protocol-Version header: {requested}"),
            "data": { "supported": supported, "requested": requested },
        },
    });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// `400` for a declared-legacy request missing its `Mcp-Session-Id`.
fn session_required_rejection() -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32002,
            "message": "the 2025-11-25 path requires an Mcp-Session-Id header (initialize first)",
        },
    });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// Whether the request's `Accept` header lists `required` as supported.
/// Media ranges are matched per RFC 9110 §12.5.1 — `*/*` and `type/*`
/// wildcards count, and parameters (`;q=…`) are ignored for the "listed as
/// supported" check the MCP transports spec makes. A missing `Accept` header
/// fails: the spec says the client MUST include one.
fn accepts(headers: &HeaderMap, required: &mime::Mime) -> bool {
    let Some(accept) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    accept
        .split(',')
        .filter_map(|part| part.trim().parse::<mime::Mime>().ok())
        .any(|range| {
            (range.type_() == mime::STAR || range.type_() == required.type_())
                && (range.subtype() == mime::STAR || range.subtype() == required.subtype())
        })
}

/// `406` + a JSON-RPC error body for a request whose `Accept` header doesn't
/// cover the response types this endpoint produces (transports spec: POST
/// clients MUST list both `application/json` and `text/event-stream`; GET
/// clients MUST list `text/event-stream`).
fn not_acceptable_rejection(detail: &str) -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": -32000, "message": format!("not acceptable: {detail}") },
    });
    (StatusCode::NOT_ACCEPTABLE, Json(body)).into_response()
}

/// Returns `Some(rejection)` if the request's `Origin` is disallowed, else `None`.
fn check_origin(policy: &OriginPolicy, headers: &HeaderMap) -> Option<Response> {
    let origin = headers.get(header::ORIGIN)?; // no Origin → non-browser → allowed
    match policy {
        OriginPolicy::Any => None,
        OriginPolicy::Allowlist(list) => {
            let origin = origin.to_str().unwrap_or_default();
            (!list.iter().any(|allowed| allowed == origin))
                .then(|| (StatusCode::FORBIDDEN, "origin not allowed").into_response())
        }
    }
}

/// Returns `Some(rejection)` if the request's `Host` is disallowed, else `None`.
/// Unlike `Origin`, `Host` is always present, so `Allowlist` mode rejects a
/// missing/unmatched `Host` — the point is to pin the server's expected host(s).
fn check_host(policy: &HostPolicy, headers: &HeaderMap) -> Option<Response> {
    match policy {
        HostPolicy::Any => None,
        HostPolicy::Allowlist(list) => {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            (!list.iter().any(|allowed| allowed == host))
                .then(|| (StatusCode::FORBIDDEN, "host not allowed").into_response())
        }
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
        Err(e) => protocol_error_response(&ProtocolError::from(e)),
    }
}

/// A malformed body has no usable id; answer `400` with a JSON-RPC parse error.
fn parse_error_response(detail: &str) -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": -32700, "message": format!("parse error: {detail}") },
    });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// Map a service/transport [`ProtocolError`] to an HTTP status + JSON-RPC error
/// body (PLAN §4.10). User `McpError`s never reach here — the dispatcher renders
/// them as `Ok` error *responses*; this is for parse/version/shutdown conditions.
fn protocol_error_response(err: &ProtocolError) -> Response {
    let status = match err {
        ProtocolError::Parse(_)
        | ProtocolError::UnsupportedVersion { .. }
        | ProtocolError::MissingCapability(_) => StatusCode::BAD_REQUEST,
        // Spec §Session Management: an expired/unknown session answers 404 so
        // the client starts over with a fresh initialize.
        ProtocolError::UnknownSession(_) => StatusCode::NOT_FOUND,
        ProtocolError::Transport(_) | ProtocolError::ServerShuttingDown => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": err.jsonrpc_code(), "message": err.to_string() },
    });
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::JsonRpcRequest;

    #[test]
    fn http_error_bridges_into_protocol_error() {
        let err = HttpError::Io(std::io::Error::other("bind failed"));
        match ProtocolError::from(err) {
            ProtocolError::Transport(msg) => assert!(msg.contains("bind failed")),
            other => panic!("expected Transport, got {other:?}"),
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn peer(socket: &str, xff: Option<&str>) -> PeerIp {
        PeerIp {
            socket: Some(ip(socket)),
            forwarded: xff.map(str::to_owned),
        }
    }

    #[test]
    fn client_ip_uses_socket_when_no_trusted_proxies() {
        // Even with an XFF header, an empty trust list ignores it (unspoofable).
        let p = peer("203.0.113.9", Some("1.2.3.4"));
        assert_eq!(p.client_ip(&[]), Some(ip("203.0.113.9")));
    }

    #[test]
    fn client_ip_ignores_xff_from_untrusted_peer() {
        let p = peer("203.0.113.9", Some("1.2.3.4"));
        assert_eq!(p.client_ip(&[ip("10.0.0.1")]), Some(ip("203.0.113.9")));
    }

    #[test]
    fn client_ip_uses_xff_behind_trusted_proxy() {
        // Peer is the trusted LB; the real client is the rightmost untrusted hop.
        let p = peer("10.0.0.1", Some("9.9.9.9, 203.0.113.7"));
        assert_eq!(p.client_ip(&[ip("10.0.0.1")]), Some(ip("203.0.113.7")));
    }

    #[test]
    fn client_ip_skips_trusted_hops_in_xff() {
        // Two trusted proxies chained: skip both, take the client.
        let p = peer("10.0.0.1", Some("203.0.113.7, 10.0.0.2"));
        let trusted = [ip("10.0.0.1"), ip("10.0.0.2")];
        assert_eq!(p.client_ip(&trusted), Some(ip("203.0.113.7")));
    }

    /// A draft-enveloped `tools/call` body and its compliant header set.
    fn draft_call(region: &str) -> JsonRpcMessage {
        JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "locate",
                "arguments": { "region": region, "n": 3, "ok": true },
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            })),
        ))
    }

    fn draft_call_headers<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all = vec![
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "tools/call"),
            ("Mcp-Name", "locate"),
        ];
        all.extend_from_slice(extra);
        all
    }

    #[test]
    fn validation_passes_a_compliant_draft_request() {
        let msg = draft_call("us-west");
        let ok = validate_request_headers(
            &msg,
            &headers(&draft_call_headers(&[
                ("Mcp-Param-region", "us-west"),
                ("Mcp-Param-n", "3"),
                ("Mcp-Param-ok", "true"),
            ])),
        );
        assert!(ok.is_none());
    }

    #[test]
    fn validation_skips_bodies_without_a_declared_version() {
        // Legacy traffic (no `_meta` version envelope) keeps the session
        // flow's tolerance — no headers required here.
        let msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "locate", "arguments": {} })),
        ));
        assert!(validate_request_headers(&msg, &headers(&[])).is_none());
    }

    #[test]
    fn validation_requires_and_matches_the_standard_headers() {
        let msg = draft_call("us-west");
        // Missing version header.
        assert!(validate_request_headers(&msg, &headers(&[])).is_some());
        // Version header not matching the body.
        assert!(
            validate_request_headers(
                &msg,
                &headers(&[
                    ("MCP-Protocol-Version", "2025-11-25"),
                    ("Mcp-Method", "tools/call"),
                    ("Mcp-Name", "locate"),
                ])
            )
            .is_some()
        );
        // Missing Mcp-Method.
        assert!(
            validate_request_headers(
                &msg,
                &headers(&[
                    ("MCP-Protocol-Version", "2026-07-28"),
                    ("Mcp-Name", "locate")
                ])
            )
            .is_some()
        );
        // Mcp-Name not matching `params.name`.
        assert!(
            validate_request_headers(
                &msg,
                &headers(&[
                    ("MCP-Protocol-Version", "2026-07-28"),
                    ("Mcp-Method", "tools/call"),
                    ("Mcp-Name", "other_tool"),
                ])
            )
            .is_some()
        );
    }

    #[test]
    fn mcp_param_headers_are_validated_never_merged() {
        let msg = draft_call("body-value");
        // A mismatching mirror is rejected…
        assert!(
            validate_request_headers(
                &msg,
                &headers(&draft_call_headers(&[("Mcp-Param-region", "header-value")])),
            )
            .is_some()
        );
        // …a Base64-sentinel-encoded matching mirror is accepted…
        let unicode = draft_call("Hello, 世界");
        assert!(
            validate_request_headers(
                &unicode,
                &headers(&draft_call_headers(&[(
                    "Mcp-Param-region",
                    "=?base64?SGVsbG8sIOS4lueVjA==?="
                )])),
            )
            .is_none()
        );
        // …a header naming no body argument is unrecognized and ignored…
        assert!(
            validate_request_headers(
                &msg,
                &headers(&draft_call_headers(&[("Mcp-Param-unknown", "x")])),
            )
            .is_none()
        );
        // …and the body is never mutated (headers are mirrors, not sources).
        let JsonRpcMessage::Request(req) = &msg else {
            unreachable!()
        };
        assert_eq!(
            req.params.as_ref().unwrap()["arguments"]["region"],
            "body-value"
        );
    }
}
