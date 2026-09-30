//! # turbomcp-transport-http
//!
//! The Streamable HTTP transport, both halves of it:
//!
//! - **`server`** (default): a single MCP endpoint served by **axum 0.8**
//!   ([`router`], [`serve_http`], [`HttpConfig`]).
//! - **`client`**: [`HttpClientTransport`], a
//!   [`Transport`](turbomcp_service::Transport) for `turbomcp-client`'s
//!   `Client`, and [`connect_http`]. **`oauth`** adds the challenge-driven
//!   OAuth 2.1 bearer source ([`oauth::OAuthSession`]).
//!
//! The two halves share the protocol's header names ([`headers`]), so they
//! can't drift apart on how either spells them.
//!
//! ## The server
//!
//! Unlike stdio (one long-lived bidirectional byte stream driven by the
//! [`serve`](turbomcp_service::serve) loop), HTTP is request/response — axum
//! owns the accept loop and concurrency. So this crate is *not* a
//! [`Transport`](turbomcp_service::Transport) impl; it is a runner that drives
//! an [`McpService`](turbomcp_service::McpService) directly: each `POST` body is decoded to a
//! [`JsonRpcMessage`](turbomcp_core::JsonRpcMessage), handed to a per-request clone of the service, and the
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
//!   requests on the legacy path, progress, log messages), or is still
//!   running after [`HttpConfig::sse_upgrade_after`] — a
//!   `200 text/event-stream` *scoped to that request*: the request-related
//!   messages as events, then the final response, which terminates the stream
//!   (transports spec §Sending Messages). Events carry no `id`, which is a
//!   MAY belonging to §Resumability and Redelivery — this endpoint does not
//!   read `Last-Event-ID`, so advertising one would promise a replay that
//!   never comes. A modern `subscriptions/listen`
//!   request yields a long-lived `200 text/event-stream` instead: the
//!   acknowledged notification first, then the opted-in change notifications.
//!   Every SSE response carries keep-alive comments (default 15s) and
//!   `X-Accel-Buffering: no` so proxies don't buffer. Closing a stream is the
//!   cancellation signal for the work it carries.
//! - **`GET {path}`** — with an `Mcp-Session-Id` header: the legacy
//!   (`2025-11-25`) server→client SSE stream for that session (list_changed,
//!   resources/updated). Without one, or on an endpoint that serves only
//!   `2026-07-28`: `405` — that revision replaced the GET stream with
//!   `subscriptions/listen`.
//! - **`DELETE {path}`** — with a [`SessionTerminator`](turbomcp_service::SessionTerminator) configured
//!   ([`HttpConfig::with_session_terminator`]): ends the `Mcp-Session-Id`
//!   session (`204`, or `404` if unknown). Without one: `405` — the
//!   `2025-11-25` spec lets a server refuse termination (sessions then expire
//!   by store eviction / idle timeout). An endpoint serving only `2026-07-28`
//!   answers `405`.
//!
//! ## Dual-stack request routing (PLAN §11)
//!
//! Modern `2026-07-28` messages are stateless (version inside the body's
//! `_meta`, or the `MCP-Protocol-Version` header) and pass through untouched;
//! an `Mcp-Session-Id` header on one is ignored, as that revision says. The
//! legacy `2025-11-25` stateful path
//! is routed from HTTP headers, and what the endpoint learns (the session, the
//! authenticated identity, the mirrored headers) is attached to the request
//! beside the message, where a client can't write:
//!
//! 1. An explicit but unsupported `MCP-Protocol-Version` header → `400`.
//! 2. Body is `initialize` → mint a session id, attach it to the message; on
//!    success the response carries it back as `Mcp-Session-Id`.
//! 3. `Mcp-Session-Id` header present → attach the session id (and, for
//!    version-less bodies, the legacy version) and dispatch; an unknown
//!    session answers `404` so the client re-initializes.
//! 4. Anything else without a session id (other than `initialize` and
//!    `server/discover`) → `400` (the legacy path requires a session).
//!
//! Every refusal carries a JSON-RPC error body (with the request's id when
//! it was read), so a client can tell this endpoint from a legacy HTTP+SSE
//! server by the body, as the 2026-07-28 fallback rules have it.
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
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod headers;

#[cfg(all(feature = "websocket", any(feature = "server", feature = "client")))]
mod ws_link;
#[cfg(all(feature = "websocket", any(feature = "server", feature = "client")))]
#[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
pub use ws_link::WsError;

#[cfg(feature = "server")]
mod server;
#[cfg(all(feature = "server", feature = "websocket"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "server", feature = "websocket"))))]
pub use server::WebSocketConfig;
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub use server::{Http, HttpConfig, HttpError, router, serve_http};

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub use client::{
    BearerSource, HttpClientError, HttpClientLimits, HttpClientTransport, connect_http,
};
#[cfg(all(feature = "client", feature = "websocket"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "client", feature = "websocket"))))]
pub use client::{WebSocketClientTransport, connect_websocket};

#[cfg(feature = "oauth")]
#[cfg_attr(docsrs, doc(cfg(feature = "oauth")))]
pub mod oauth;
