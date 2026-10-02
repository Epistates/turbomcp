//! [`HttpConfig`] and the Origin/Host policies it carries.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderValue, Method, header};
use ipnet::IpNet;
use tokio_util::task::TaskTracker;
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};
use turbomcp_core::ProtocolVersion;
use turbomcp_service::{CancellationToken, HttpAuthenticator, RateLimiter, SessionTerminator};

#[cfg(feature = "websocket")]
use super::WebSocketConfig;
use super::resume::EventStore;
use crate::headers;

/// Default keep-alive comment interval — short enough to outlive common
/// proxy/LB idle timeouts (often 30–60s).
pub(super) const DEFAULT_SSE_KEEPALIVE: Duration = Duration::from_secs(15);

/// Default time a request runs before its response becomes an SSE stream.
pub(super) const DEFAULT_SSE_UPGRADE_AFTER: Duration = Duration::from_secs(5);

/// How long a detached, resumable `GET` stream waits to be resumed.
pub(super) const DEFAULT_DETACHED_STREAM_TTL: Duration = Duration::from_secs(5 * 60);

/// Where a request's `Origin` header is checked against (DNS-rebinding guard).
#[derive(Clone, Debug)]
pub(super) enum OriginPolicy {
    /// Reject any request whose `Origin` isn't in this list. An empty list lets
    /// only `Origin`-less (non-browser) clients through — the secure default.
    Allowlist(Vec<String>),
    /// Accept any `Origin` (development only).
    Any,
}

impl OriginPolicy {
    /// The CORS layer for the origins this policy admits, if it admits any.
    ///
    /// An allowed origin is only ever a browser, and a browser can't call a
    /// cross-origin endpoint without CORS, so the two are one setting. The
    /// preflight answer lists `Authorization` by name: a `*` in
    /// `Access-Control-Allow-Headers` does not cover it (Fetch standard), so
    /// the permissive layer this replaces broke bearer auth, the MCP
    /// authorization spec's only mechanism, in every browser. Request headers
    /// are mirrored so each tool's `Mcp-Param-*` names pass, and the headers a
    /// client has to read (`Mcp-Session-Id`, `WWW-Authenticate`,
    /// `Retry-After`) are exposed.
    pub(super) fn cors_layer(&self) -> Option<CorsLayer> {
        let origins = match self {
            Self::Any => AllowOrigin::any(),
            Self::Allowlist(list) if list.is_empty() => return None,
            Self::Allowlist(list) => {
                AllowOrigin::list(list.iter().filter_map(|o| HeaderValue::from_str(o).ok()))
            }
        };
        Some(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers(AllowHeaders::mirror_request())
                .expose_headers([
                    headers::SESSION_ID,
                    headers::PROTOCOL_VERSION,
                    header::WWW_AUTHENTICATE,
                    header::RETRY_AFTER,
                ]),
        )
    }
}

/// Where a request's `Host` header is checked against — DNS-rebinding defense in
/// depth, complementing [`OriginPolicy`] for non-browser clients that can spoof
/// `Host` (the `Origin` guard only covers browsers).
#[derive(Clone, Debug)]
pub(super) enum HostPolicy {
    /// Accept any `Host` (the default — suited to deployments behind a proxy or
    /// load balancer that rewrites `Host`).
    Any,
    /// Reject any request whose `Host` isn't in this list. Lets a server that
    /// knows its expected host(s) refuse a spoofed `Host`.
    Allowlist(Vec<String>),
}

/// The well-known path RFC 9728 Protected Resource Metadata is served at.
pub(super) const RESOURCE_METADATA_PATH: &str = "/.well-known/oauth-protected-resource";

/// Configuration for the HTTP endpoint. Construct with [`HttpConfig::new`] and
/// chain the builder methods.
#[derive(Clone)]
pub struct HttpConfig {
    pub(super) max_concurrent_requests: usize,
    pub(super) max_streams: usize,
    pub(super) max_streams_per_client: usize,
    pub(super) request_timeout: Duration,
    pub(super) shutdown_timeout: Duration,
    pub(super) path: String,
    pub(super) max_body_bytes: usize,
    pub(super) origins: OriginPolicy,
    pub(super) hosts: HostPolicy,
    pub(super) shutdown: CancellationToken,
    pub(super) sse_keepalive: Duration,
    pub(super) sse_upgrade_after: Duration,
    pub(super) authenticator: Option<Arc<dyn HttpAuthenticator>>,
    pub(super) rate_limiter: Option<Arc<dyn RateLimiter>>,
    pub(super) ip_rate_limiter: Option<Arc<dyn RateLimiter>>,
    pub(super) session_terminator: Option<Arc<dyn SessionTerminator>>,
    pub(super) trusted_proxies: Vec<IpNet>,
    pub(super) supported_versions: Option<Vec<ProtocolVersion>>,
    pub(super) health_path: Option<String>,
    pub(super) calls: TaskTracker,
    pub(super) event_store: Option<Arc<dyn EventStore>>,
    pub(super) sse_polling: Option<SsePolling>,
    pub(super) detached_stream_ttl: Duration,
    #[cfg(feature = "websocket")]
    pub(super) websocket: Option<WebSocketConfig>,
}

/// Server-initiated polling of resumable streams (`2025-11-25` §Sending
/// Messages to the Server, SEP-1699): see [`HttpConfig::with_sse_polling`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SsePolling {
    /// How long a response stream's connection stays open.
    pub close_after: Duration,
    /// The `retry` the client waits before polling the stream back.
    pub retry: Duration,
}

impl SsePolling {
    /// Close each connection after `close_after`, asking the client to come
    /// back after `retry`.
    #[must_use]
    pub fn new(close_after: Duration, retry: Duration) -> Self {
        Self { close_after, retry }
    }
}

impl core::fmt::Debug for HttpConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut f = f.debug_struct("HttpConfig");
        f.field("path", &self.path)
            .field("max_concurrent_requests", &self.max_concurrent_requests)
            .field("max_streams", &self.max_streams)
            .field("max_streams_per_client", &self.max_streams_per_client)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("origins", &self.origins)
            .field("hosts", &self.hosts)
            .field("sse_keepalive", &self.sse_keepalive)
            .field("sse_upgrade_after", &self.sse_upgrade_after)
            .field("authenticator", &self.authenticator.is_some())
            .field("rate_limiter", &self.rate_limiter.is_some())
            .field("ip_rate_limiter", &self.ip_rate_limiter.is_some())
            .field("session_terminator", &self.session_terminator.is_some())
            .field("trusted_proxies", &self.trusted_proxies)
            .field("supported_versions", &self.supported_versions)
            .field("health_path", &self.health_path)
            .field("event_store", &self.event_store.is_some())
            .field("sse_polling", &self.sse_polling)
            .field("detached_stream_ttl", &self.detached_stream_ttl);
        #[cfg(feature = "websocket")]
        f.field("websocket", &self.websocket);
        f.finish()
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 1024,
            max_streams: 1024,
            max_streams_per_client: 64,
            request_timeout: Duration::from_secs(60),
            shutdown_timeout: Duration::from_secs(30),
            path: "/mcp".to_owned(),
            max_body_bytes: 1 << 20, // 1 MiB
            origins: OriginPolicy::Allowlist(Vec::new()),
            hosts: HostPolicy::Any,
            shutdown: CancellationToken::new(),
            sse_keepalive: DEFAULT_SSE_KEEPALIVE,
            sse_upgrade_after: DEFAULT_SSE_UPGRADE_AFTER,
            authenticator: None,
            rate_limiter: None,
            ip_rate_limiter: None,
            session_terminator: None,
            trusted_proxies: Vec::new(),
            supported_versions: None,
            health_path: None,
            calls: TaskTracker::new(),
            event_store: None,
            sse_polling: None,
            detached_stream_ttl: DEFAULT_DETACHED_STREAM_TTL,
            #[cfg(feature = "websocket")]
            websocket: None,
        }
    }
}

impl HttpConfig {
    /// The revisions this endpoint serves, for the `MCP-Protocol-Version`
    /// check: "if the server receives a request with an invalid **or
    /// unsupported** `MCP-Protocol-Version`, it MUST respond with `400 Bad
    /// Request`".
    ///
    /// Defaults to what the server serves, so `#[server(protocols("2025-06-18"))]`
    /// refuses a `2025-11-25` header instead of advertising a version it does
    /// not serve. Set it only to narrow the endpoint further.
    #[must_use]
    pub fn with_supported_versions(mut self, versions: Vec<ProtocolVersion>) -> Self {
        self.supported_versions = Some(versions);
        self
    }

    /// Bound requests in flight (default 1024): from admission until the
    /// response has been sent, including a tool call whose response streams.
    /// Over it, a request gets `503` + `Retry-After`.
    ///
    /// Long-lived streams (a legacy `GET` stream, a `subscriptions/listen`
    /// stream) don't count here once open: they have
    /// [their own budget](Self::max_streams).
    #[must_use]
    pub fn max_concurrent_requests(mut self, limit: usize) -> Self {
        self.max_concurrent_requests = limit.max(1);
        self
    }

    /// Bound open long-lived streams, legacy `GET` and `subscriptions/listen`
    /// together (default 1024). Over it, a new stream gets `503` +
    /// `Retry-After`.
    #[must_use]
    pub fn max_streams(mut self, limit: usize) -> Self {
        self.max_streams = limit.max(1);
        self
    }

    /// Bound the long-lived streams one caller may hold open (default 64):
    /// per authenticated subject, or per client IP for anonymous callers
    /// (see [`with_trusted_proxies`](Self::with_trusted_proxies)). Over it, a
    /// new stream gets `429`. Without this, one client could hold every
    /// stream the endpoint allows. A caller with neither an identity nor a
    /// peer address is held to [`max_streams`](Self::max_streams) only.
    #[must_use]
    pub fn max_streams_per_client(mut self, limit: usize) -> Self {
        self.max_streams_per_client = limit.max(1);
        self
    }
    /// Deadline to be admitted, authenticate, read the request, and send
    /// response headers (default 60 s). It does not bound how long a request
    /// runs: a request still working after
    /// [`sse_upgrade_after`](Self::sse_upgrade_after) has its headers sent
    /// then, and keeps its connection open with keep-alives until it answers.
    /// When the deadline passes first, the client gets `504` with a JSON-RPC
    /// error body.
    #[must_use]
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }
    /// Maximum drain time after shutdown is requested.
    #[must_use]
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

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
    /// Browsers on it get CORS headers too.
    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        match &mut self.origins {
            OriginPolicy::Allowlist(list) => list.push(origin.into()),
            OriginPolicy::Any => {}
        }
        self
    }

    /// Accept requests from any `Origin` (development only), with CORS
    /// headers for all of them.
    #[must_use]
    pub fn allow_any_origin(mut self) -> Self {
        self.origins = OriginPolicy::Any;
        self
    }

    /// Add an allowed `Host`: a host (`mcp.example.com`, `localhost`), which
    /// matches it on any port, or a host and port (`localhost:8080`), which
    /// matches only that port. By default any `Host` is accepted; once at least one
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

    /// Provide a cancellation token; firing it triggers axum's graceful shutdown.
    #[must_use]
    pub fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = shutdown;
        self
    }

    /// A clone of the configured shutdown token (a fresh, never-fired token by
    /// default), for callers coordinating their own teardown.
    #[must_use]
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// How long a request may run before its response becomes an SSE stream
    /// (default 5 s). A request that answers sooner, and sends nothing on the
    /// way, gets a plain JSON response.
    ///
    /// A tool that works for minutes without a word used to hold its
    /// response headers back all that time, and the request deadline (or any
    /// proxy's idle timeout in front, typically 60 s) cut it off with a bare
    /// `504` and cancelled the work. Once the stream is open, keep-alive
    /// comments hold the connection until the result arrives. Keep this well
    /// under [`request_timeout`](Self::request_timeout) and any proxy's idle
    /// timeout.
    ///
    /// On `2026-07-28`, errors that carry an HTTP status (`-32601` is `404`)
    /// only get it when they arrive before the upgrade; after it, the status
    /// line has been sent and the error travels in the stream.
    #[must_use]
    pub fn sse_upgrade_after(mut self, after: Duration) -> Self {
        self.sse_upgrade_after = after;
        self
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
    /// identity-derived [`RateKey`](turbomcp_service::RateKey) — per authenticated subject when the
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

    /// Rate-limit by source IP *before* anything else happens: before the
    /// body is read, before a bearer token is verified. Every request is
    /// charged against [`RateKey::Ip`](turbomcp_service::RateKey::Ip) (or [`RateKey::Global`](turbomcp_service::RateKey::Global) with no peer
    /// address), and an over-budget one gets `429` + `Retry-After`.
    ///
    /// [`with_rate_limiter`](Self::with_rate_limiter) charges authenticated
    /// callers per subject, which only works once the token is verified, so a
    /// flood of bad tokens never reaches it: each still costs a signature
    /// check (and a body parse) and is never throttled. This is the cheap
    /// first gate for that. Keep its quota generous, since callers behind one
    /// NAT share it.
    #[must_use]
    pub fn with_ip_rate_limiter(mut self, rate_limiter: Arc<dyn RateLimiter>) -> Self {
        self.ip_rate_limiter = Some(rate_limiter);
        self
    }

    /// Trust these proxies to set `X-Forwarded-For`: addresses or CIDR ranges
    /// (anything that converts into an [`IpNet`], so a plain [`IpAddr`](std::net::IpAddr) works
    /// too). When the direct socket peer is one of them, the client IP used
    /// for rate limiting is taken from the right of the `X-Forwarded-For`
    /// chain (the first hop not itself trusted) instead of the proxy's own
    /// address. Every `X-Forwarded-For` line counts, in order: a proxy that
    /// appends a new line rather than extending the client's is common, and
    /// reading only the first let a client name any address it liked.
    /// Spoofable if you list an address that isn't actually your proxy; list
    /// only your real front ends. Empty (default) means the raw socket peer is
    /// always used.
    #[must_use]
    pub fn with_trusted_proxies<I, N>(mut self, proxies: I) -> Self
    where
        I: IntoIterator<Item = N>,
        N: Into<IpNet>,
    {
        self.trusted_proxies = proxies.into_iter().map(Into::into).collect();
        self
    }

    /// Serve a health check at `path` (e.g. `/healthz`): `GET` answers `200`
    /// `{"status":"ok"}` while the endpoint serves and `503`
    /// `{"status":"draining"}` once shutdown begins, so a load balancer stops
    /// sending new traffic before connections are closed. It needs no
    /// authentication and passes the Origin and Host checks untouched (a load
    /// balancer sends neither), but it is still subject to the request
    /// limit, so an endpoint too busy to admit it reads as unhealthy.
    #[must_use]
    pub fn with_health_check(mut self, path: impl Into<String>) -> Self {
        self.health_path = Some(path.into());
        self
    }

    /// Make response streams on a session (`2025-06-18` / `2025-11-25`)
    /// resumable, keeping their events in `store`
    /// ([`InMemoryEventStore`](crate::InMemoryEventStore) is the bundled one).
    ///
    /// Such a stream is primed with an event id and every event carries one;
    /// a client that loses the connection sends `GET` with `Last-Event-ID`
    /// and is caught up on what it missed, then follows the rest live. A call
    /// on a session already runs on after its client disconnects; this is
    /// what lets its response still reach the client. Off by default, and
    /// then no event ids are sent: an id is a promise of replay.
    #[must_use]
    pub fn with_event_store(mut self, store: Arc<dyn EventStore>) -> Self {
        self.event_store = Some(store);
        self
    }

    /// Don't hold resumable streams open (needs an
    /// [event store](Self::with_event_store)): each response stream's
    /// connection closes after `polling.close_after`, with a `retry` field,
    /// and the client polls it back with `Last-Event-ID` ("the server MAY
    /// close the connection (without terminating the SSE stream) at any time
    /// in order to avoid holding a long-lived connection"). Nothing is lost:
    /// the stream keeps recording while no connection carries it. Useful
    /// behind proxies and load balancers that cut idle or long connections
    /// anyway. Off by default.
    #[must_use]
    pub fn with_sse_polling(mut self, polling: SsePolling) -> Self {
        self.sse_polling = Some(polling);
        self
    }

    /// How long a session's `GET` stream keeps recording after its client
    /// disconnects, waiting to be resumed (default 5 minutes; needs an
    /// [event store](Self::with_event_store)). Past it, the stream ends and
    /// what was recorded for it is dropped.
    #[must_use]
    pub fn detached_stream_ttl(mut self, ttl: Duration) -> Self {
        self.detached_stream_ttl = ttl;
        self
    }

    /// Also accept WebSocket connections, on `websocket`'s path (feature
    /// `websocket`). Every guard this config sets applies to the upgrade.
    #[cfg(feature = "websocket")]
    #[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
    #[must_use]
    pub fn with_websocket(mut self, websocket: WebSocketConfig) -> Self {
        self.websocket = Some(websocket);
        self
    }

    /// Honor client-initiated session termination with `terminator`: a
    /// `DELETE` carrying an `Mcp-Session-Id` ends that `2025-11-25` session
    /// (dropping its state and subscription routes) and answers `204`; an
    /// unknown session answers `404`. A server built by `turbomcp-server`
    /// supplies its own, so this is for replacing it. Without one, `DELETE`
    /// answers `405` (the spec permits a server refusing termination).
    #[must_use]
    pub fn with_session_terminator(mut self, terminator: Arc<dyn SessionTerminator>) -> Self {
        self.session_terminator = Some(terminator);
        self
    }
}
