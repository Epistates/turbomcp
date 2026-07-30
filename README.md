# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

A ground-up Rust SDK for the [Model Context Protocol](https://modelcontextprotocol.io) —
both halves of the protocol, server **and** client — with a macro-driven,
zero-boilerplate surface and strict spec compliance as a feature.

> **Status: `4.0.0-alpha.1` — a prerelease for community testing.** v4 is a
> from-scratch rewrite of TurboMCP; the stable line is `3.x`. Edition 2024,
> MSRV 1.88. It passes the official MCP conformance suite (43/43) and
> interoperates with the official Rust SDK in both directions. The draft
> protocol revision it speaks (`2026-07-28`) is synced to the upstream
> `2026-07-28-RC` tag, and an RC is not a freeze — draft wire details may still
> move. `2025-06-18` and `2025-11-25` support is stable.
> **Found something broken or unergonomic? Please open an issue.**

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

The two make a different central bet.

**rmcp models the protocol once and branches where revisions differ:** one set
of model types organized by concept, a negotiated `ProtocolVersion` string, and
conditional checks at the points where behaviour changes. That is lighter, and
it reaches further back — rmcp 2.2 knows five revisions (`2024-11-05`,
`2025-03-26`, `2025-06-18`, `2025-11-25`, `2026-07-28`) where TurboMCP serves
three. **If you need clients on `2024-11-05` or `2025-03-26`, use rmcp; this
project cannot serve them.**

**TurboMCP generates a separate wire type set per revision and converts between
them.** Handlers speak version-neutral types; each revision's shapes are
generated from that revision's published schema, and the conversions between
them destructure exhaustively — so adding a field to one revision's wire is a
compile error until someone decides what the other does with it. Fewer
revisions, but the differences between them are checked by the compiler rather
than by a reviewer.

Beyond that, TurboMCP ships things you would otherwise write yourself:
capabilities *derived* from the markers present (advertisement can't drift from
implementation); per-RPC typed contexts, so calling elicitation from a
`list_tools` handler doesn't compile; `Composite` for mounting several servers
as one; `with_visibility` for deciding per caller which components exist;
`tower::Layer` middleware at the frame seam; and a `no_std`, `wasm32`-portable
foundation.

Both forbid `unsafe`, both are edition 2024, both cover server and client.
rmcp is Apache-2.0; this is MIT.

**Pick rmcp** for the official implementation, the older protocol revisions, or
the smallest dependency surface. **Pick TurboMCP** for the macro surface,
multi-revision support the type system enforces, or the composition, visibility,
and auth seams above.

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
| `turbomcp-protocol` | MCP protocol: neutral types, `2025-11-25` + draft wire shapes, version dispatch |
| `turbomcp-service` | The `tower`-shaped protocol seam, transport trait, shared RPC middleware |
| `turbomcp-server` | Handler registry, dispatcher, `ServerBuilder`, graceful shutdown |
| `turbomcp-client` | Typed client: handshake, version negotiation, neutral API |
| `turbomcp-transport-stdio` / `-http` / `-ws` | Transport implementations |
| `turbomcp-auth` | OAuth 2.1 resource-server auth (bearer validation, RFC 9728) |
| `turbomcp-telemetry` | OpenTelemetry tracing (W3C `_meta` propagation, PII-safe spans) |
| `turbomcp-ext-tasks` | Draft Tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663) |

## Verification

Compliance is tested, not asserted:

- **Official conformance suite** — the vendored
  `@modelcontextprotocol/conformance` harness drives a full-featured TurboMCP
  server over Streamable HTTP: on the pinned stable harness (0.1.16),
  **47 checks — 43 pass, 0 fail, 4 informational**; the next-generation
  `0.2.0-alpha` harness (52 checks) also passes clean
  (`crates/turbomcp-conformance`).
- **Cross-SDK interop** — a TurboMCP client drives an official-Rust-SDK
  (rmcp 2.2) server and vice-versa, in-process
  (`crates/turbomcp-interop`).
- **≈520 tests across the workspace** (plus 86 more re-run against the
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
