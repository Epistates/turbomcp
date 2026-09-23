# TurboMCP Server

[![Crates.io](https://img.shields.io/crates/v/turbomcp-server.svg)](https://crates.io/crates/turbomcp-server)
[![Documentation](https://docs.rs/turbomcp-server/badge.svg)](https://docs.rs/turbomcp-server)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Server framework for the Model Context Protocol. Provides the `McpHandlerExt`
entry points, the `ServerBuilder` for runtime transport selection, typed
middleware (`McpMiddleware` / `MiddlewareStack`), configuration types
(`ServerConfig`, `ProtocolConfig`, rate limits, connection limits, origin
validation), and the JSON-RPC router shared by every transport.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Server Builder](#server-builder)
- [Server Configuration](#server-configuration)
- [HTTP Authorization](#http-authorization)
- [Protocol Version Negotiation](#protocol-version-negotiation)
- [Visibility](#visibility)
- [Middleware](#middleware)
- [Transports](#transports)
- [Feature Flags](#feature-flags)
- [Related Crates](#related-crates)

## Overview

`turbomcp-server` is a Layer 5 crate that turns any `McpHandler`
(implemented by the `#[server]` macro, `CompositeHandler`, or a hand-written
type) into a running MCP server. It owns:

- Entry points (`McpHandlerExt::run`, `run_stdio`, `run_http`, `run_tcp`,
  `run_unix`, `run_websocket`, `handle_request`).
- The `ServerBuilder` fluent API for runtime transport and config selection.
- The JSON-RPC router (`router::route_request` et al.) shared by all
  transports.
- `ServerConfig` and its validated builder (`try_build`) plus `ProtocolConfig`
  for version negotiation.
- The typed `McpMiddleware` trait and `MiddlewareStack<H>` composition wrapper.
- Progressive disclosure (`VisibilityLayer`) and server composition
  (`CompositeHandler`).
- MCP authorization for the Streamable HTTP transport (`HttpAuthorization`,
  `BearerTokenValidator`), with the `http` feature.

Token validators (OAuth 2.1 / JWT / API keys) live in `turbomcp-auth`.
Telemetry lives in `turbomcp-telemetry`. This crate does not bundle them.

## Quick Start

Any type that implements `McpHandler` (the `#[server]` macro generates one for
you) gets the `run*` and `builder()` methods automatically via blanket impls.
The macros come from the `turbomcp` facade crate:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(name = "calculator", version = "1.0.0")]
impl Calculator {
    /// Add two numbers
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 { a + b }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // STDIO by default
    Calculator.run().await
}
```

## Server Builder

`ServerBuilder<H>` is obtained via `McpServerExt::builder()` (blanket impl
on every `McpHandler`). The methods available on it:

| Method | Description |
|---|---|
| `.transport(Transport)` | Select transport (default: `Transport::Stdio`) |
| `.with_rate_limit(u32, Duration)` | Enable token-bucket rate limiting (per client) |
| `.with_connection_limit(usize)` | Cap concurrent connections across TCP/HTTP/WS/Unix |
| `.with_graceful_shutdown(Duration)` | Wait up to this duration for in-flight requests on shutdown (HTTP transport) |
| `.with_max_message_size(usize)` | Reject messages larger than this (default: 10 MB) |
| `.with_protocol(ProtocolConfig)` | Configure protocol version negotiation |
| `.with_allowed_origin(impl Into<String>)` | Allow a specific HTTP origin |
| `.with_origin_validation(OriginValidationConfig)` | Replace the full origin config |
| `.allow_localhost_origins(bool)` | Accept/deny localhost origins |
| `.allow_any_origin(bool)` | Disable origin checks entirely |
| `.with_config(ServerConfig)` | Apply a `ServerConfig` (protocol, rate limit, connection limits, required capabilities, message size, origin validation) |
| `.serve()` | Start the server (async, blocks until shutdown) |
| `.into_axum_router()` | Return an `axum::Router` for BYO server integration (requires `http`) |
| `.into_service()` | Return a Tower service (requires `http`) |
| `.handler()` / `.into_handler()` | Borrow / consume the underlying handler |

With `Calculator` from the Quick Start and the `http` feature:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    Calculator.builder()
        .transport(Transport::http("0.0.0.0:8080"))
        .with_rate_limit(100, Duration::from_secs(1))
        .with_connection_limit(1000)
        .with_graceful_shutdown(Duration::from_secs(30))
        .serve()
        .await
}
```

### BYO server (Axum integration)

```rust
use axum::{Router, routing::get};
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mcp = Calculator.builder().into_axum_router();

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(mcp);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

## Server Configuration

`ServerConfig` is constructed through `ServerConfig::builder()`. Fields:

| Field | Type | Default |
|---|---|---|
| `protocol` | `ProtocolConfig` | See [Protocol Version Negotiation](#protocol-version-negotiation) |
| `rate_limit` | `Option<RateLimitConfig>` | `None` |
| `connection_limits` | `ConnectionLimits` | 1000 per transport |
| `required_capabilities` | `RequiredCapabilities` | none |
| `max_message_size` | `usize` | 10 MB |
| `origin_validation` | `OriginValidationConfig` | `allow_localhost = true`, no explicit origins, `allow_any = false`, `allow_missing_origin = false`, `cors = false` |
| `http_sessions` | `HttpSessionConfig` | 1 hour idle timeout, 10,000 sessions |
| `authorization` | `Option<HttpAuthorization>` (`http` feature) | `None` |

A request with no `Origin` header from a non-loopback address is refused
unless `allow_missing_origin(true)` is set. CLIs, agent runtimes, and other
servers send no `Origin`, so a networked server that serves them needs it,
paired with authorization.

Use `.build()` for an infallible build with defaults, or `.try_build()` to
validate. `try_build()` returns `ConfigValidationError` when:

- `max_message_size` is below 1024 bytes
- `RateLimitConfig::max_requests` is 0
- `RateLimitConfig::window` is `Duration::ZERO`
- All four fields of `ConnectionLimits` are 0

```rust
use std::time::Duration;
use turbomcp_server::{ServerConfig, RateLimitConfig};

let config = ServerConfig::builder()
    .max_message_size(1024 * 1024)
    .rate_limit(RateLimitConfig::new(100, Duration::from_secs(1)))
    .try_build()
    .expect("invalid server configuration");
```

## HTTP Authorization

With the `http` feature, `ServerConfig::builder().authorization(...)` makes
the Streamable HTTP transport an OAuth 2.1 protected resource, as MCP's
authorization spec describes. The server publishes RFC 9728 Protected
Resource Metadata under `/.well-known/oauth-protected-resource`, answers a
request without a valid bearer token `401` with a `WWW-Authenticate`
challenge pointing at it (`403 insufficient_scope` for a missing scope), and
puts the validated `Principal` on each request's context.

Tokens are checked by a `BearerTokenValidator`, which must accept only tokens
issued for this server. `turbomcp_auth::server::JwtBearerValidator` (feature
`mcp-http-server`) validates JWTs, audience included; a hand-written one looks
like this:

```rust
use turbomcp::prelude::*;
use turbomcp_core::auth::Principal;
use turbomcp_server::{BearerRejection, BearerTokenValidator, HttpAuthorization, ValidationFuture};

/// Accepts one static token. Real servers validate a JWT, audience included.
struct StaticToken(String);

impl BearerTokenValidator for StaticToken {
    fn validate<'a>(&'a self, token: &'a str) -> ValidationFuture<'a> {
        Box::pin(async move {
            if token == self.0 {
                Ok(Principal::new("service-account"))
            } else {
                Err(BearerRejection::InvalidToken("unknown token".into()))
            }
        })
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    let authorization = HttpAuthorization::new(
        "https://mcp.example.com/mcp", // this server's canonical URL
        "https://auth.example.com",    // where clients get tokens
        StaticToken("s3cret".into()),
    )
    .with_scopes_supported(["mcp:tools"]);

    let config = ServerConfig::builder()
        .authorization(authorization)
        .allow_missing_origin(true)
        .build();

    turbomcp_server::transport::http::run_with_config(&Calculator, "0.0.0.0:8080", &config).await
}
```

## Protocol Version Negotiation

`ProtocolConfig` controls which MCP spec versions the server speaks.
Fields: `preferred_version: ProtocolVersion`, `supported_versions:
Vec<ProtocolVersion>`, `allow_fallback: bool`.

**Default**: `preferred_version = ProtocolVersion::LATEST` (`2025-11-25`),
`supported_versions = ProtocolVersion::STABLE.to_vec()` (`2025-06-18` and
`2025-11-25`), `allow_fallback = true`. A client asking for a supported
version gets it, and responses are filtered through that version's adapter.
A client asking for any other version is offered the preferred one, as the
lifecycle spec requires, and decides for itself whether to continue.

`ProtocolConfig::strict(version)` speaks only `version`. It still answers a
client that asked for another version, offering `version` rather than
refusing the handshake, because the spec does not permit a refusal.
`ProtocolConfig::multi_version()` builds the default explicitly.

```rust
use turbomcp::prelude::*;
use turbomcp_server::ProtocolVersion;

#[tokio::main]
async fn main() -> McpResult<()> {
    // Speak only 2025-11-25
    Calculator.builder()
        .with_protocol(ProtocolConfig::strict(ProtocolVersion::LATEST))
        .serve()
        .await?;

    // Explicit multi-version (same as default)
    Calculator.builder()
        .with_protocol(ProtocolConfig::multi_version())
        .serve()
        .await
}
```

`ProtocolConfig::negotiate(client_version)` returns the negotiated
`ProtocolVersion`. It returns `None` only when `allow_fallback` has been set to
`false` and the client asked for an unsupported version, which makes the
server refuse the handshake with `-32602`; the spec does not allow that, so
leave `allow_fallback` on.

## Visibility

`VisibilityLayer` wraps any `McpHandler` and filters `tools/list`,
`resources/list`, `resources/templates/list`, and `prompts/list`. Disabled
tools, resources, and prompts are also rejected on call/read/get paths as not
found; hidden components stay callable but are omitted from list responses.

Use it for both human UX and LLM-facing AX: expose only the tools relevant to a
deployment profile, disable unsafe operations, and keep client context focused.

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    let config = VisibilityConfig::new()
        .with_allowed_tools(["search", "read_note", "list_notes"])
        .with_disabled_tools(["delete_note", "reindex_all"])
        .with_hidden_tools(["advanced_graph_query"])
        .require_read_only_tools();

    // `Notes` is your #[server] type
    let server = VisibilityLayer::new(Notes).with_visibility_config(config);
    server.run_stdio().await
}
```

For config files, map user choices into `VisibilityConfig`: exact tool
allowlists reduce context load, disabled tools remove risky operations, hidden
tools stay callable without consuming `tools/list` context, and
`require_read_only_tools()` only exposes tools annotated with `readOnlyHint:
true`.

Direct call/read/get authorization is registry-backed. The registry is populated
from list responses and lazily initialized on first direct use, so dispatch does
not re-enumerate components on every request. Dynamic servers that add or remove
advertised components at runtime can call `refresh_component_registry()` or
`clear_component_registry()` on the layer.

## Middleware

Middleware is typed around the MCP operation set. Implement `McpMiddleware`
and layer it onto any `McpHandler` via `MiddlewareStack`:

```rust
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, MiddlewareStack, Next};

struct Logging;

impl McpMiddleware for Logging {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(tool = name, "calling");
            next.call_tool(name, args, ctx).await
        })
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // MiddlewareStack wraps a handler; it itself implements McpHandler,
    // so it participates in the same builder / transport pipeline.
    let stack = MiddlewareStack::new(Calculator).with_middleware(Logging);
    stack.builder().serve().await
}
```

The trait's other hooks (`on_list_tools`, `on_list_resources`,
`on_list_prompts`, `on_read_resource`, `on_get_prompt`, etc.) all have
pass-through default implementations — override only the ones you need.

## Transports

Runtime transport selection is done through `Transport`. Each variant is
gated by the matching feature flag.

| Constructor | Feature flag | Notes |
|---|---|---|
| `Transport::stdio()` | `stdio` | Default; line-based JSON-RPC over stdin/stdout (Claude Desktop) |
| `Transport::http(addr)` | `http` | Streamable HTTP (Axum): POST for requests, SSE for server messages, `Mcp-Session-Id` sessions; served at `/` and `/mcp` |
| `Transport::websocket(addr)` | `websocket` | Bidirectional JSON-RPC; depends on `http` |
| `Transport::tcp(addr)` | `tcp` | Line-framed JSON-RPC over TCP |
| `Transport::unix(path)` | `unix` | Line-framed JSON-RPC over Unix domain socket |

Each `McpHandler` also has direct `run_stdio` / `run_http` / `run_websocket`
/ `run_tcp` / `run_unix` methods (feature-gated) via `McpHandlerExt`, plus
`handle_request(Value, RequestContext)` for serverless-style one-shot use.

## Feature Flags

| Feature | Description | Default |
|---|---|---|
| `stdio` | STDIO transport | ✅ |
| `http` | HTTP transport (Axum) | ❌ |
| `websocket` | WebSocket transport (implies `http`) | ❌ |
| `tcp` | TCP transport | ❌ |
| `unix` | Unix domain socket transport | ❌ |
| `channel` | In-process channel transport | ❌ |
| `all-transports` | `stdio` + `http` + `websocket` + `tcp` + `unix` + `channel` | ❌ |
| `full` | Alias for `all-transports` | ❌ |
| `experimental-tasks` | Opt into experimental Tasks API (SEP-1686) | ❌ |

## Related Crates

- **[turbomcp](../turbomcp/)** — Main SDK that re-exports this crate's public API
- **[turbomcp-core](../turbomcp-core/)** — `McpHandler`, `McpError`, JSON-RPC primitives
- **[turbomcp-protocol](../turbomcp-protocol/)** — Protocol implementation, session management
- **[turbomcp-transport](../turbomcp-transport/)** — Transport re-export hub
- **[turbomcp-auth](../turbomcp-auth/)** — OAuth 2.1 / JWT / API keys (optional)
- **[turbomcp-telemetry](../turbomcp-telemetry/)** — OpenTelemetry / Prometheus (optional)

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) Rust SDK for the Model Context Protocol.*
