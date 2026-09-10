# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

A ground-up Rust SDK for the [Model Context Protocol](https://modelcontextprotocol.io) —
both halves of the protocol, server **and** client — with a macro-driven,
zero-boilerplate surface and strict spec compliance as a feature.

> **Status: `4.0.0-alpha.4` — a prerelease for community testing.** v4 is a
> from-scratch rewrite of TurboMCP; the stable line is `3.x`. Edition 2024,
> MSRV 1.88. It interoperates with the official Rust SDK in both directions, on
> both revisions, and both halves are scored against the official MCP conformance
> suite: 231 successful server assertions and 488 distinct successful client
> scenario/check pairs using pinned client fixture corrections, with zero failures, skips, or warnings. All three advertised revisions (`2025-06-18`, `2025-11-25`,
> `2026-07-28`) are dated and frozen; `2026-07-28` is generated from the
> released `schema/2026-07-28/`, not the RC.
> **Found something broken or unergonomic? Please open an issue.**

## What you get

- **One macro defines a server.** `#[server]` over an `impl` block turns
  `#[tool]` / `#[resource]` / `#[prompt]` methods into a fully-wired MCP server.
  The macro generates schema derivation code; schema values are initialized once at runtime and cloned for callers, and
  the advertised capabilities are *derived* from which markers are present — they
  can't drift from the implementation.
- **Three protocol revisions, one handler.** The same server answers
  `2025-06-18`, `2025-11-25`, and `2026-07-28`. Your handlers speak
  version-neutral types; the version-specific wire shapes are conversions, not
  signature changes — including dropping, per session, the fields a revision
  predates. Pin the set with `#[server(protocols("2025-11-25", …))]`.
- **Transports behind one builder.** stdio (default), Streamable HTTP (axum),
  and WebSocket. `MyServer.run_stdio()`, `.run_http(addr, cfg)`, or
  `turbomcp::ws::serve_websocket(listener, factory)`.
- **The client too.** A typed `Client` runs the handshake, negotiates the
  version, and speaks the same neutral API — interoperating with the official
  Rust SDK (rmcp) in both directions.
- **Production seams.** OAuth 2.1 on both halves (resource-server bearer
  validation and the client auth-code + PKCE flow), identity-keyed rate
  limiting, OpenTelemetry tracing + metrics, progress/logging, subscriptions,
  response caching (SEP-2549), and bidirectional elicitation — each opt-in
  behind a feature flag.

## How this relates to `rmcp`, the official Rust SDK

[`rmcp`](https://github.com/modelcontextprotocol/rust-sdk) is the official SDK,
maintained in the `modelcontextprotocol` organization. It is the reasonable
default, and this project is tested against it — cross-SDK interop tests run in
both directions, a TurboMCP client against an rmcp server and the reverse, on
every change.

TurboMCP's interoperability tests pin `rmcp` 3.2. TurboMCP serves three
revisions (`2025-06-18`, `2025-11-25`, `2026-07-28`) using separate generated
wire types and exhaustive conversions to a neutral handler API. It does not
serve `2024-11-05` or `2025-03-26`.

The distinguishing APIs are macro-derived capabilities, typed per-RPC contexts,
composition, caller-specific visibility, and Tower middleware. Conformance and
interoperability are compatibility evidence; they do not establish performance
superiority over another SDK. See [deployment and migration](docs/DEPLOYMENT.md)
for the limits and security contracts.

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

See the [`turbomcp` crate README](crates/turbomcp/README.md) for the full API
tour (tools/resources/prompts, structured output, HTTP, feature flags) and the
[`examples/`](crates/turbomcp/examples/).

## Workspace layout

The SDK is a Cargo workspace; the `turbomcp` facade re-exports the pieces most
users need, so a typical dependency is just `turbomcp`.

| Crate | Role |
|---|---|
| `turbomcp` | Main SDK facade — re-exports, prelude, examples |
| `turbomcp-macros` | `#[server]` / `#[tool]` / `#[resource]` / `#[prompt]` |
| `turbomcp-core` | `no_std` foundation: `McpError`, `ProtocolVersion`, JSON-RPC, `_meta` |
| `turbomcp-codec` | Wire codec: bytes ↔ `JsonRpcMessage` (serde_json baseline, opt-in SIMD via sonic-rs) |
| `turbomcp-protocol` | MCP protocol: neutral types, date-versioned wire shapes, version dispatch |
| `turbomcp-service` | The `tower`-shaped protocol seam, transport trait, shared RPC middleware |
| `turbomcp-server` | Handler registry, dispatcher, `ServerBuilder`, graceful shutdown |
| `turbomcp-client` | Typed client: handshake, version negotiation, neutral API |
| `turbomcp-transport-stdio` / `-http` / `-ws` | Transport implementations |
| `turbomcp-auth` | OAuth 2.1 resource-server auth (bearer validation, RFC 9728) |
| `turbomcp-telemetry` | OpenTelemetry tracing (W3C `_meta` propagation, PII-safe spans) |
| `turbomcp-ext-tasks` | Draft Tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663) |

## Verification

Compliance is tested, not asserted:

- **Official conformance suite, both halves** — the
  `@modelcontextprotocol/conformance` harness runs in both directions on both
  scored revisions. As the *server*, it drives a full-featured TurboMCP server
  over Streamable HTTP: **236 checks, 231 pass, 0 fail, 5 informational**. As the
  *client*, it stands up a deliberately awkward mock server per scenario and
  referees what our client did on the wire: **488 distinct successful scenario/check pairs, 0 failures**, including the
  OAuth scenarios through the public `OAuthSession` coordinator. The client gate
  uses [hash-pinned fixture corrections](crates/turbomcp-conformance/fixtures/README.md)
  and reports zero failures, skips, or warnings. The unmodified upstream mode
  remains available with its original fixture limitations. Neither side has
  failure waivers (`crates/turbomcp-conformance`).
- **Cross-SDK interop** — a TurboMCP client drives an official-Rust-SDK
  (rmcp 3.2) server and vice-versa, in-process, on `2025-11-25` *and* the
  stateless `2026-07-28` (`crates/turbomcp-interop`).
- **Workspace regression tests** (also run against the
  `no_std` foundation configs) — dual-version dispatch, transport hardening
  (Origin/auth/size caps/idle reaping), handler-panic containment, MRTR
  elicitation, tasks (including in-execution input), subscriptions, pagination,
  response caching, auth negative paths, client failure semantics against
  misbehaving servers, and byte-level codec interchangeability
  (serde_json ↔ sonic-rs).
- **Fuzzing + supply chain** — cargo-fuzz targets for every untrusted-input
  decoder (JSON-RPC codec, `Mcp-Param` header sentinel, URI templates, and a
  sonic-vs-serde differential), run out of band via `just fuzz`; `cargo-deny`
  (advisories/bans/licenses/sources) runs in CI on every push.
- **wasm-portable foundation** — `turbomcp-core`/`-codec`/`-protocol` build
  `no_std` for `wasm32-unknown-unknown` on every gate run.

## Migrating from v3

The macro surface is intentionally source-compatible for the common case; see
[`crates/turbomcp/MIGRATION.md`](crates/turbomcp/MIGRATION.md) for the v3 → v4
deltas.

## License

MIT
