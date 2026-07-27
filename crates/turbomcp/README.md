# turbomcp

A ground-up Rust SDK for the [Model Context Protocol](https://modelcontextprotocol.io),
both halves of the protocol — server **and** client — with a macro-driven,
zero-boilerplate surface and strict spec compliance as a feature.

> **Status: `4.0.0-alpha.1` — a prerelease for community testing.** A
> ground-up rewrite of `turbomcp` for the v4 major version; the stable line is
> `3.x`. Edition 2024, MSRV 1.88. Passes the official MCP conformance suite
> with zero failures and interoperates with the official Rust SDK (rmcp 2.x)
> in both directions, verified in-repo. The draft protocol revision
> (`2026-07-28`) tracks the pre-freeze spec; `2025-06-18` and `2025-11-25`
> support is stable.

## What you get

- **One macro defines a server.** `#[server]` over an `impl` block turns
  `#[tool]` / `#[resource]` / `#[prompt]` methods into a fully-wired MCP server.
  JSON schemas are generated from your function signatures at compile time, and
  the advertised capabilities are *derived* from which markers are present — they
  can't drift from the implementation.
- **Three protocol revisions, one handler.** The same server answers
  `2025-06-18`, `2025-11-25`, and the `2026-07-28` draft. Your handlers speak
  version-neutral types; the version-specific wire shapes are conversions, not
  signature changes — including dropping, per session, the fields a revision
  predates. Pin the set with `#[server(protocols("2025-11-25", …))]`.
- **Transports behind one builder.** stdio (default), Streamable HTTP (axum),
  and WebSocket. `MyServer.run_stdio()`, `MyServer.into_server().run_http(addr,
  cfg)`, or `turbomcp::ws::serve_websocket(listener, dispatcher)`.
- **The client too.** A typed `Client` runs the handshake, negotiates the
  version, and speaks the same neutral API — interoperating with the official
  Rust SDK (rmcp) both directions. `call_tool` transparently drives task-shaped
  results (including mid-task `input_required`) to completion.
- **Middleware is `tower`.** The dispatcher *is* a `tower::Service<JsonRpcMessage>`,
  so cross-cutting concerns are ordinary `Layer`s — one `call` for every method
  under every transport, and `ServiceBuilder` / `timeout` / `ConcurrencyLimit`
  compose onto an MCP server unchanged. No hook list to keep in sync with the
  protocol.
- **Production seams.** OAuth 2.1 on both halves (resource-server bearer
  validation and the client auth-code + PKCE flow), identity-keyed rate
  limiting, OpenTelemetry tracing + metrics, progress/logging, subscriptions,
  response caching (SEP-2549), and bidirectional elicitation (MRTR) — each
  opt-in behind a feature flag.

## Quickstart

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Hello;

#[server(name = "hello", version = "1.0.0")]
impl Hello {
    /// Say hello to someone.
    #[tool(description = "Say hello to someone")]
    async fn hello(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {name}!"))
    }
}

#[tokio::main]
async fn main() -> Result<(), turbomcp::ProtocolError> {
    // Logs MUST go to stderr — stdout carries the MCP protocol framing.
    Hello.run_stdio().await
}
```

Serve the same server over Streamable HTTP instead (feature `http`):

```rust,ignore
use turbomcp::http::{HttpConfig, ServeHttp};

Hello.into_server()
    .run_http("127.0.0.1:8080".parse()?, HttpConfig::new())
    .await?;
```

## Tools, resources, prompts

```rust,ignore
#[server(name = "docs", version = "1.0.0")]
impl Docs {
    /// A tool: arguments come from the signature; the schema is generated.
    #[tool(description = "Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> String { format!("{}", a + b) }

    /// A resource at a fixed URI (resources/list + resources/read).
    #[resource("config://app")]
    async fn config(&self) -> McpResult<String> { Ok(r#"{"debug":false}"#.into()) }

    /// A prompt template; its arguments are the function arguments.
    #[prompt]
    async fn summarize(&self, text: String) -> McpResult<String> {
        Ok(format!("Summarize:\n\n{text}"))
    }
}
```

### Naming, metadata, and protocol selection

A tool's, prompt's, or resource's wire name defaults to the Rust method name;
set `name = "…"` when the two should not be coupled (renaming a Rust method is
otherwise a breaking change for clients) or when the wire name isn't a valid
Rust identifier. Tool names are checked at compile time against the spec's
rules — 1–128 characters of ASCII letters, digits, `_`, `-`, and `.` — and
against each other, so a name real clients would reject or a silently shadowed
duplicate never reaches a release.

```rust,ignore
#[tool(name = "search.web", description = "Search the web")]
async fn search_web(&self, q: String) -> String { … }
```

All three markers also take `title = "…"` (the human-facing display name), and
`#[resource]` takes `mime_type = "…"` — what a client needs to decide how to
render the bytes:

```rust,ignore
#[resource(
    "config://app",
    name = "app-config",
    title = "Application configuration",
    mime_type = "application/json",
)]
async fn config(&self) -> McpResult<String> { … }
```

All three also take `tags("…", …)`, which categorizes a component for catalog
policy — which tools a deployment or a caller should be offered. Tags ride in
the component's `_meta` (`io.turbomcp/tags`), so they survive every protocol
revision and components a sub-server contributes carry their own; read them
back with `turbomcp::tags`. They describe, they don't enforce:
`#[tool(scopes("admin"))]` is the authorization mechanism.

```rust,ignore
#[tool(tags("admin", "dangerous"), scopes("admin"))]
async fn wipe(&self, ctx: &CallToolContext) -> McpResult<String> { … }
```

A server answers both protocol revisions by default. Pin it to the frozen
stable one — the draft's wire shapes can still change before it freezes — with
`protocols(…)`; an excluded version is refused with `-32004` plus the list of
versions that *are* served:

```rust,ignore
#[server(name = "prod", version = "1.0.0", protocols("2025-11-25"))]
```

Tools return `String`/`&str`, any numeric or `bool` scalar, `()` (empty
success), `Json<T>` (structured output — see below), or a
`neutral::CallToolResult` — each optionally wrapped in `McpResult<_>`. A
returned `McpError` becomes a tool-level error (`CallToolResult { isError }`) the
model can see — not a transport error.

### Structured output

Return `Json<T>` (where `T: Serialize + schemars::JsonSchema`) to produce a typed
result: the value goes in `structuredContent` with a JSON text mirror for
backward compatibility, and the macro generates the tool's `outputSchema` from
`T`.

```rust,ignore
#[derive(serde::Serialize, turbomcp::schemars::JsonSchema)]
struct Stats { count: u64, mean: f64 }

#[tool(description = "Compute stats")]
async fn stats(&self) -> Json<Stats> { Json(Stats { count: 3, mean: 1.5 }) }
```

## Feature flags

| Feature | Enables |
|---|---|
| *(default)* | stdio transport (always linked) |
| `http` | Streamable HTTP transport (axum); the client's HTTP transport when `client` is on |
| `websocket` | WebSocket transport (bidirectional, non-spec) → `turbomcp::ws` (`WsConfig`: Origin policy, bearer auth, size caps, keepalive) |
| `client` | the typed `Client` + `ConnectMode` negotiation |
| `auth` | OAuth 2.1 resource-server auth (bearer validation, RFC 9728 metadata) |
| `client-oauth` | the OAuth 2.1 *client* flow (auth-code + PKCE, discovery, registration, refresh) → `turbomcp::auth::client` |
| `telemetry` | OpenTelemetry tracing + metrics (`TraceContextLayer`, `MetricsLayer`, W3C `_meta` propagation, PII-safe spans) |
| `ext-tasks` | the draft Tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663) |
| `simd` | SIMD JSON (sonic-rs) as the default codec on native x86_64/aarch64; byte-compatible with the serde_json baseline |

## Examples

In [`examples/`](examples/) — run with `cargo run -p turbomcp --example <name>`:

| Example | Shows |
|---|---|
| `hello_world` | the minimal one-tool server |
| `calculator` | several tools; infallible vs fallible returns |
| `stateful` | shared `Arc<RwLock<…>>` state across requests |
| `validation` | handler-body validation → tool-level errors |
| `resources_prompts` | the non-tool surface: resources + prompts |
| `structured_output` | `Json<T>` → `structuredContent` + generated `outputSchema` |
| `elicitation` | asking the user for input (MRTR + legacy inline) |
| `middleware` | `tower::Layer`s over the dispatcher: one that audits, one that refuses |
| `dual_transport` | one server over stdio **and** HTTP (`--features http`) |
| `tasks` | the draft Tasks extension (`--features ext-tasks`) |
| `client` | the other half: a client that spawns `hello_world` and drives it (`--features client`) |

## Migrating from v3

See [`MIGRATION.md`](MIGRATION.md) for the v3 → v4 deltas.

## License

MIT
