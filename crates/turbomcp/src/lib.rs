//! # turbomcp
//!
//! The TurboMCP v4 SDK facade: a single crate that re-exports the layered
//! workspace crates and the `#[server]` / `#[tool]` / `#[resource]` / `#[prompt]`
//! macros, plus a [`prelude`] for the common imports.
//!
//! ```
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct Hello;
//!
//! #[server(name = "hello", version = "1.0.0")]
//! impl Hello {
//!     /// Say hello to someone.
//!     #[tool]
//!     async fn hello(&self, name: String) -> McpResult<String> {
//!         Ok(format!("Hello, {name}!"))
//!     }
//! }
//!
//! # async fn run() -> Result<(), turbomcp::ProtocolError> {
//! // Logs MUST go to stderr — stdout carries the MCP protocol framing.
//! Hello.run_stdio().await
//! # }
//! ```
//!
//! ## Tool return types
//!
//! A `#[tool]` returns `String`/`&str`, any numeric or `bool` scalar, `()`
//! (empty success), [`Json<T>`] (structured output — the value lands in
//! `structuredContent` and the macro generates the tool's `outputSchema` from
//! `T`), [`Image`] / [`Audio`] (base64 `data` + `mime_type` → a content
//! block), or a [`neutral::CallToolResult`] — each optionally wrapped in
//! [`McpResult`]. A returned [`McpError`] becomes a *tool-level* error
//! (`CallToolResult { isError: true }`) the model can read and correct, not a
//! transport error.
//!
//! ```
//! use turbomcp::prelude::*;
//!
//! #[derive(serde::Serialize, turbomcp::schemars::JsonSchema)]
//! struct Stats { count: u64, mean: f64 }
//!
//! #[derive(Clone)]
//! struct Kitchen;
//!
//! #[server(name = "kitchen-sink", version = "1.0.0")]
//! impl Kitchen {
//!     /// A bare scalar becomes a text content block.
//!     #[tool]
//!     async fn add(&self, a: i64, b: i64) -> i64 { a + b }
//!
//!     /// `Json<T>` becomes `structuredContent` + a generated `outputSchema`.
//!     #[tool]
//!     async fn stats(&self) -> Json<Stats> { Json(Stats { count: 3, mean: 1.5 }) }
//!
//!     /// `Image`/`Audio` become a single image/audio content block.
//!     #[tool]
//!     async fn chart(&self) -> Image {
//!         Image { data: String::new(), mime_type: "image/png".into() }
//!     }
//!
//!     /// A returned `McpError` is a tool-level error, not a transport error.
//!     #[tool]
//!     async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
//!         if b == 0.0 {
//!             return Err(McpError::invalid_params("b must be non-zero"));
//!         }
//!         Ok(a / b)
//!     }
//! }
//! ```
//!
//! Note: on the `2025-11-25` wire `structuredContent` must be a JSON object,
//! so a `Json<T>` serializing to a scalar or array carries its value in the
//! text mirror only there; the `2026-07-28` wire accepts any JSON value.
//!
//! ## RPC middleware
//!
//! Cross-cutting concerns wrap the server as [`tower::Layer`]s over
//! `Service<McpRequest>` — one `call` for every method under every
//! transport: `MyServer.into_server().layer(TracingLayer).serve(stdio())`.
//! Adding a layer costs nothing else; the runtime still wires sessions,
//! `DELETE` and graceful shutdown on every transport. `tower` itself
//! is re-exported as [`tower`] so the `Layer` you write is the one the SDK
//! expects. Start from [`TracingLayer`] (the shape in 30 lines) and the
//! [`middleware` example](https://github.com/Epistates/turbomcp/blob/main/crates/turbomcp/examples/middleware.rs).
//!
//! Auth and rate limiting are *not* RPC middleware here: both need the HTTP
//! request a JSON-RPC frame no longer carries, so they are transport-level seams
//! (`HttpConfig::with_authenticator` / `with_rate_limiter`, feature `http`).
//! Per-tool authorization is `#[tool(scopes(…))]`.
#![forbid(unsafe_code)]
// docs.rs builds with `--cfg docsrs` on nightly so every feature-gated item
// renders with the feature that unlocks it.
#![cfg_attr(docsrs, feature(doc_cfg))]
// Every example in these docs is a real doctest — they are the API contract
// users read first, so they compile or the build fails.
#![warn(missing_docs)]

// ---- foundation -------------------------------------------------------------

pub use turbomcp_core::{
    Claims, ConnectionId, Extensions, Identity, Implementation, JsonRpcError, JsonRpcMessage,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, LogLevel, McpError, McpRequest,
    McpResult, ObservedHeaders, ProtocolVersion, RequestContext, RequestId, SessionId, codes,
};

/// Version-stable, handler-facing types (the surface user handlers speak).
pub use turbomcp_protocol::neutral;

/// The MCP method-name constants (`methods::request::TOOLS_CALL`, …). Match on
/// these rather than string literals in RPC middleware — a literal is where a
/// renamed method silently stops matching.
pub use turbomcp_protocol::methods;

/// Reading the component tags that `#[tool(tags(…))]` / `#[resource(tags(…))]`
/// / `#[prompt(tags(…))]` write into a component's `_meta`.
pub use turbomcp_server::tags;

/// Progressive disclosure: which components a given caller may see — and
/// therefore reach. Install with
/// [`ServerBuilder::with_visibility`](ServerBuilder::with_visibility).
///
/// A hidden component is refused *exactly as one that does not exist*, since a
/// distinct "forbidden" answer would disclose what the policy is hiding.
/// [`Visibility`] covers the two common cases (hide by tag, hide what the
/// caller lacks the scopes for); implement [`VisibilityPolicy`] — a bare
/// closure will do — for anything else, including per-session unlocking keyed
/// on your own store.
pub use turbomcp_server::visibility;

pub use turbomcp_server::{ComponentKind, Visibility, VisibilityPolicy, VisibleComponent};

// ---- service seam + codec ---------------------------------------------------

pub use turbomcp_core::codec::{Codec, CodecError, DefaultCodec, SerdeJsonCodec};
pub use turbomcp_service::{
    CancellationToken, Delivery, McpService, Peer, PeerClosed, Pipe, ProtocolError, Serve,
    ServeConfig, ServerHandle, SessionStreams, Transport, serve, serve_with,
};

/// RPC middleware: [`tower::Layer`]s over the `Service<McpRequest>` seam,
/// applying identically under stdio, HTTP, and WebSocket.
///
/// [`TracingLayer`] wraps each RPC in a `tracing` span naming the method — it is
/// also the smallest complete worked example of the shape (see its source).
/// Compose it, or your own, around the built dispatcher:
///
/// ```no_run
/// use turbomcp::prelude::*;
/// use turbomcp::TracingLayer;
///
/// # #[derive(Clone)]
/// # struct MyServer;
/// # #[server(name = "my-server", version = "1.0.0")]
/// # impl MyServer {
/// #     #[tool]
/// #     async fn ping(&self) -> String { "pong".into() }
/// # }
/// # async fn run() -> Result<(), turbomcp::ProtocolError> {
/// // What `run_stdio()` does, with one layer added.
/// MyServer.into_server().layer(TracingLayer).serve(stdio()).await
/// # }
/// ```
///
/// See the [`middleware` example](https://github.com/Epistates/turbomcp/blob/main/crates/turbomcp/examples/middleware.rs)
/// for an observing layer and a short-circuiting one, and
/// [`MIGRATION.md`](https://github.com/Epistates/turbomcp/blob/main/crates/turbomcp/MIGRATION.md)
/// for the mapping from v3's `McpMiddleware` hooks.
pub use turbomcp_service::TracingLayer;

/// The service [`TracingLayer`] produces.
pub use turbomcp_service::Tracing;

/// Re-export of [`tower`], version-matched to the one behind [`McpService`], so
/// middleware written against it composes without a duplicate-crate mismatch.
pub use tower;

// ---- server -----------------------------------------------------------------

/// Server composition: mount several servers under prefixes and serve them as
/// one. Tools and prompts are namespaced `{prefix}__{name}`; resource URIs are
/// left alone (a URI is already a namespace, and rewriting one makes it a lie).
///
/// ```no_run
/// use turbomcp::prelude::*;
/// use turbomcp::{Composite, Implementation};
///
/// # #[derive(Clone)] struct Weather;
/// # #[server(name = "weather", version = "1.0.0")]
/// # impl Weather { #[tool] async fn forecast(&self) -> String { "sunny".into() } }
/// # #[derive(Clone)] struct News;
/// # #[server(name = "news", version = "1.0.0")]
/// # impl News { #[tool] async fn headlines(&self) -> String { "…".into() } }
/// # async fn run() -> McpResult<()> {
/// // Serves `weather__forecast` and `news__headlines`.
/// let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
///     .mount("weather", Weather.into_server())?
///     .mount("news", News.into_server())?
///     .into_server()
///     .build();
/// # Ok(()) }
/// ```
pub use turbomcp_server::{Composite, CompositeServer};

pub use turbomcp_server::{
    Audio, CachePolicies, CallToolContext, ClientHandle, CompleteContext, GetPromptContext, Image,
    IntoCallToolResult, IntoGetPromptResult, IntoReadResourceResult, IntoServerBuilder, Json,
    LegacySessionAdapter, ListPromptsContext, ListResourceTemplatesContext, ListResourcesContext,
    ListToolsContext, LogSender, McpServerCore, MethodRouter, ProgressReporter,
    ReadResourceContext, Server, ServerBuilder, ServerNotifier, SessionBackend, SessionState,
    SessionStore, TaskBackend, TaskError, TaskOutcome, TaskSnapshot, TaskStatus, TaskStore,
    UriTemplate, UriTemplateError, VersionDispatcher, WithCompletions, WithPrompts, WithResources,
    WithTools,
};

/// Re-export of [`schemars`] for deriving `JsonSchema` on `#[tool]` argument
/// structs and [`Json`] structured-output types, so downstream crates don't pin
/// a separate `schemars` version. Use `#[derive(turbomcp::schemars::JsonSchema)]`.
pub use schemars;

// ---- transports -------------------------------------------------------------

/// stdio: [`stdio()`] is the transport to [`serve`](ServerBuilder::serve) on,
/// dual-stack like the macro's `run_stdio()` (every revision the server
/// accepts, stateful clients included):
///
/// ```no_run
/// use turbomcp::prelude::*;
///
/// # #[derive(Clone)]
/// # struct MyServer;
/// # #[server(name = "my-server", version = "1.0.0")]
/// # impl MyServer {
/// #     #[tool]
/// #     async fn ping(&self) -> String { "pong".into() }
/// # }
/// # async fn run() -> Result<(), turbomcp::ProtocolError> {
/// MyServer.into_server().with_logging().serve(stdio()).await
/// # }
/// ```
///
/// Wrap it in a [`Pipe`] to set a [`ServeConfig`] (shutdown token, drain and
/// write timeouts, concurrency). [`serve_stdio`] is the raw driver: it serves
/// a service exactly as given, so a bare dispatcher there answers only
/// stateless `2026-07-28` clients.
pub use turbomcp_service::io::{LineTransport, serve_stdio, serve_stdio_with, stdio};

/// Streamable HTTP transport (axum 0.8). Enable with the `http` feature.
///
/// Serve a builder on [`Http`](http::Http), with or without middleware; the
/// runtime wires supported revisions, `DELETE` session termination and
/// graceful `subscriptions/listen` close either way:
///
/// ```no_run
/// use turbomcp::prelude::*;
/// use turbomcp::http::{Http, HttpConfig};
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool]
///     async fn ping(&self) -> String { "pong".into() }
/// }
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let addr = "127.0.0.1:8080".parse()?;
/// MyServer.into_server().serve(Http::bind(addr).config(HttpConfig::new())).await?;
///
/// // With RPC middleware (e.g. the telemetry `TraceContextLayer`):
/// MyServer.into_server().layer(turbomcp::TracingLayer).serve(Http::bind(addr)).await?;
/// # Ok(())
/// # }
/// ```
///
/// [`router`](http::router) builds the axum `Router` to mount inside a larger
/// app.
///
/// WebSocket (feature `websocket`, not an MCP-spec transport) is a route on
/// the same endpoint, behind the same guards:
/// `HttpConfig::new().with_websocket(WebSocketConfig::new("/ws"))`. Connect
/// to it with [`client::connect_websocket`](crate::client) (features
/// `client` + `websocket`).
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub mod http {
    pub use turbomcp_service::SessionTerminator;
    pub use turbomcp_transport_http::{Http, HttpConfig, HttpError, router, serve_http};

    #[cfg(feature = "websocket")]
    #[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
    pub use turbomcp_transport_http::WebSocketConfig;
}

/// OAuth 2.1 resource-server auth: bearer-token validation + RFC 9728 metadata.
/// Enable with the `auth` feature, then protect an HTTP endpoint with
/// [`HttpConfig::with_authenticator`](http::HttpConfig::with_authenticator).
#[cfg(feature = "auth")]
#[cfg_attr(docsrs, doc(cfg(feature = "auth")))]
pub use turbomcp_auth as auth;

/// The HTTP authentication seam (implemented by [`auth::ResourceServer`]).
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub use turbomcp_service::{AuthDecision, AuthFuture, HttpAuthenticator};

/// The HTTP rate-limiting seam + the in-process `governor`-backed default.
/// Apply with [`HttpConfig::with_rate_limiter`](http::HttpConfig::with_rate_limiter).
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub use turbomcp_service::{GovernorRateLimiter, RateKey, RateLimiter};

/// OpenTelemetry observability: the [`TraceContextLayer`](telemetry::TraceContextLayer)
/// (W3C trace continuation over `_meta` + PII-safe identity spans), the
/// [`MetricsLayer`](telemetry::MetricsLayer) (request count / duration /
/// in-flight, labeled by method + version + outcome), and an optional OTLP
/// export pipeline (traces + metrics). Enable with the `telemetry` feature.
#[cfg(feature = "telemetry")]
#[cfg_attr(docsrs, doc(cfg(feature = "telemetry")))]
pub use turbomcp_telemetry as telemetry;

/// The MCP client: [`client::ClientBuilder`] runs the handshake + version
/// negotiation, then [`client::Client`] speaks the typed [`neutral`] API.
/// Enable with the `client` feature; add `http` for
/// [`HttpClientTransport`](client::HttpClientTransport) and
/// [`connect_http`](client::connect_http).
#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client {
    pub use turbomcp_client::*;

    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use turbomcp_transport_http::{
        BearerSource, HttpClientError, HttpClientLimits, HttpClientTransport, connect_http,
    };

    #[cfg(feature = "websocket")]
    #[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
    pub use turbomcp_transport_http::{WebSocketClientTransport, WsError, connect_websocket};

    /// Coordinated HTTP OAuth authorization and token refresh.
    #[cfg(feature = "client-oauth")]
    #[cfg_attr(docsrs, doc(cfg(feature = "client-oauth")))]
    pub use turbomcp_transport_http::oauth;
}

/// The draft Tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663):
/// register [`ext_tasks::TasksExtension`] with `ServerBuilder::with_extension`
/// to answer `tools/call` with an async task handle. Enable with the
/// `ext-tasks` feature.
#[cfg(feature = "ext-tasks")]
#[cfg_attr(docsrs, doc(cfg(feature = "ext-tasks")))]
pub use turbomcp_ext_tasks as ext_tasks;

// ---- macros -----------------------------------------------------------------

pub use turbomcp_macros::{completion, mcp_header, prompt, resource, server, tool};

/// Support items referenced by `#[server]`-generated code. **Not** a stable API
/// — do not depend on it directly; it exists only so generated code has a single
/// rooted path (`::turbomcp::__macros::…`) for its dependencies.
#[doc(hidden)]
pub mod __macros {
    pub use schemars;
    pub use serde;
    pub use serde_json;

    pub use turbomcp_core::meta::keys::SCOPES as SCOPES_META_KEY;
    pub use turbomcp_core::meta::keys::TAGS as TAGS_META_KEY;
    pub use turbomcp_core::{McpError, McpResult};
    pub use turbomcp_protocol::neutral;
    pub use turbomcp_server::__macro_support::{
        assert_header_param, close_object_schema, extend_object_schema, mark_mcp_header,
        match_uri_template, normalize_input_schema,
    };
}

/// The common imports for building a server.
pub mod prelude {
    pub use crate::neutral;
    pub use turbomcp_core::{Implementation, LogLevel, McpError, McpResult, RequestContext};
    pub use turbomcp_server::{
        Audio, CallToolContext, CompleteContext, GetPromptContext, Image, IntoServerBuilder, Json,
        ListPromptsContext, ListResourceTemplatesContext, ListResourcesContext, ListToolsContext,
        McpServerCore, ReadResourceContext, ServerBuilder, WithCompletions, WithPrompts,
        WithResources, WithTools,
    };
    pub use turbomcp_service::io::stdio;

    /// Streamable HTTP to [`serve`](ServerBuilder::serve) on (feature `http`).
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use crate::http::{Http, HttpConfig};

    pub use turbomcp_macros::{completion, mcp_header, prompt, resource, server, tool};
}
