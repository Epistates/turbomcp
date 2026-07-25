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
#![forbid(unsafe_code)]
// docs.rs builds with `--cfg docsrs` on nightly so every feature-gated item
// renders with the feature that unlocks it.
#![cfg_attr(docsrs, feature(doc_cfg))]
// Every example in these docs is a real doctest — they are the API contract
// users read first, so they compile or the build fails.
#![warn(missing_docs)]

// ---- foundation -------------------------------------------------------------

pub use turbomcp_core::{
    Claims, Identity, Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, LogLevel, McpError, McpResult, ProtocolVersion, RequestContext, RequestId,
};

/// Version-stable, handler-facing types (the surface user handlers speak).
pub use turbomcp_protocol::neutral;

// ---- service seam + codec ---------------------------------------------------

pub use turbomcp_codec::{Codec, CodecError, DefaultCodec, SerdeJsonCodec};
pub use turbomcp_service::{
    CancellationToken, McpService, ProtocolError, ServeConfig, Transport, serve, serve_with,
};

// ---- server -----------------------------------------------------------------

pub use turbomcp_server::{
    Audio, CachePolicies, CallToolContext, ClientHandle, CompleteContext, GetPromptContext, Image,
    IntoCallToolResult, IntoGetPromptResult, IntoReadResourceResult, IntoServerBuilder, Json,
    LegacySessionAdapter, ListPromptsContext, ListResourceTemplatesContext, ListResourcesContext,
    ListToolsContext, LogSender, McpServerCore, MethodRouter, ProgressReporter,
    ReadResourceContext, ServerBuilder, ServerNotifier, SessionBackend, SessionState, SessionStore,
    TaskBackend, TaskError, TaskSnapshot, TaskStatus, TaskStore, VersionDispatcher,
    WithCompletions, WithPrompts, WithResources, WithTools,
};

/// Re-export of [`schemars`] for deriving `JsonSchema` on `#[tool]` argument
/// structs and [`Json`] structured-output types, so downstream crates don't pin
/// a separate `schemars` version. Use `#[derive(turbomcp::schemars::JsonSchema)]`.
pub use schemars;

// ---- transports -------------------------------------------------------------

pub use turbomcp_transport_stdio::{serve_stdio, serve_stdio_with, stdio};

/// Streamable HTTP transport (axum 0.8). Enable with the `http` feature.
///
/// The one-liner is [`ServeHttp::run_http`](http::ServeHttp::run_http) on a
/// builder — it builds the dispatcher, wires session termination (`DELETE`)
/// automatically, and serves:
///
/// ```no_run
/// use turbomcp::prelude::*;
/// use turbomcp::http::{HttpConfig, ServeHttp};
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
/// MyServer.into_server().run_http("127.0.0.1:8080".parse()?, HttpConfig::new()).await?;
/// # Ok(())
/// # }
/// ```
///
/// For full control — in particular to wrap the dispatcher in RPC middleware
/// such as the telemetry [`TraceContextLayer`](crate::telemetry::TraceContextLayer)
/// (feature `telemetry`) — build the service yourself and call
/// [`serve_http`](http::serve_http). Note that this path does *not* auto-wire
/// `DELETE` session termination; pass
/// [`HttpConfig::with_session_terminator`](http::HttpConfig::with_session_terminator)
/// if you need it.
///
/// ```no_run
/// # use turbomcp::prelude::*;
/// use turbomcp::http::{HttpConfig, serve_http};
///
/// # #[derive(Clone)]
/// # struct MyServer;
/// # #[server(name = "my-server", version = "1.0.0")]
/// # impl MyServer {
/// #     #[tool]
/// #     async fn ping(&self) -> String { "pong".into() }
/// # }
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// # let addr = "127.0.0.1:8080".parse()?;
/// // …or `SomeLayer::new().layer(…)` around this to add RPC middleware.
/// let service = MyServer.into_server().build();
/// serve_http(addr, service, HttpConfig::new()).await?;
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub mod http {
    use std::net::SocketAddr;
    use std::sync::Arc;

    pub use turbomcp_service::SessionTerminator;
    pub use turbomcp_transport_http::{HttpConfig, HttpError, router, serve_http};

    use turbomcp_server::{McpServerCore, ServerBuilder};

    /// One-call HTTP serving for a [`ServerBuilder`] (the value
    /// `MyServer.into_server()` produces).
    pub trait ServeHttp {
        /// Build this server's dispatcher and serve it over Streamable HTTP on
        /// `addr` until `config`'s shutdown token fires.
        ///
        /// Session termination (`DELETE`) is wired automatically from the built
        /// dispatcher, so the endpoint honors client-initiated termination by
        /// default. To compose RPC middleware first, build the dispatcher
        /// yourself and call [`serve_http`] instead.
        fn run_http(
            self,
            addr: SocketAddr,
            config: HttpConfig,
        ) -> impl std::future::Future<Output = Result<(), HttpError>> + Send;
    }

    impl<S> ServeHttp for ServerBuilder<S>
    where
        S: McpServerCore + Clone + Send + Sync + 'static,
    {
        async fn run_http(self, addr: SocketAddr, config: HttpConfig) -> Result<(), HttpError> {
            let dispatcher = self.build();
            let config = config.with_session_terminator(Arc::new(dispatcher.session_terminator()));
            // Graceful teardown: when the shutdown token fires, drop the live
            // `subscriptions/listen` registrations (the transport ends the
            // listen SSE streams itself off the same token — the RC sends no
            // closing response).
            let closer = dispatcher.clone();
            let shutdown = config.shutdown_token();
            tokio::spawn(async move {
                shutdown.cancelled().await;
                closer.close_subscriptions();
            });
            serve_http(addr, dispatcher, config).await
        }
    }
}

/// WebSocket transport (bidirectional, non-spec convenience). Enable with the
/// `websocket` feature. Serve with
/// [`ws::serve_websocket`] over a `TcpListener` (see [`ws::WsConfig`] for
/// Origin policy, bearer auth, limits, and keepalive), or connect a client
/// transport with [`ws::connect`].
#[cfg(feature = "websocket")]
#[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
pub use turbomcp_transport_ws as ws;

/// OAuth 2.1 resource-server auth: bearer-token validation + RFC 9728 metadata.
/// Enable with the `auth` feature, then protect an HTTP endpoint with
/// [`HttpConfig::with_authenticator`](http::HttpConfig::with_authenticator).
#[cfg(feature = "auth")]
#[cfg_attr(docsrs, doc(cfg(feature = "auth")))]
pub use turbomcp_auth as auth;

/// The HTTP authentication seam (implemented by [`auth::ResourceServer`]).
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub use turbomcp_service::{AuthDecision, HttpAuthenticator};

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
/// Enable with the `client` feature.
#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub use turbomcp_client as client;

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

    pub use turbomcp_core::{McpError, McpResult};
    pub use turbomcp_protocol::neutral;
    pub use turbomcp_server::__macro_support::{
        close_object_schema, mark_mcp_header, match_uri_template, normalize_input_schema,
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
    pub use turbomcp_transport_stdio::serve_stdio;

    /// The HTTP one-liner `builder.run_http(addr, config)` (feature `http`).
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use crate::http::ServeHttp;

    pub use turbomcp_macros::{completion, mcp_header, prompt, resource, server, tool};
}
