# Migrating from v3 to v4

v4 is a ground-up rewrite shipped as the same `turbomcp` crate at a new major
version (`4.x`; the stable line is `3.x`). The macro surface — `#[server]`,
`#[tool]`, `#[resource]`, `#[prompt]` over a `Clone` struct — is intentionally
source-compatible for the common case, so simple servers port with little more
than a version bump. The deltas below are the places where v4 deliberately
differs.

## Crate & imports

The crate name and prelude import are unchanged from v3:

```rust
use turbomcp::prelude::*;
```

The prelude still brings in the macros, `McpResult`, `McpError`, the per-RPC
context types, and `run_stdio`.

## Tool return types

A `#[tool]` may return `String`/`&str`, any numeric or `bool` scalar (→ text),
`()` (empty success), `Json<T>` (structured output), `Image` / `Audio`
(base64 `data` + `mime_type` → an image/audio content block), or a
`neutral::CallToolResult` — each optionally wrapped in `McpResult<_>`. Bare
scalars work as in v3:

```rust
// v3 and v4
#[tool(description = "Add")] async fn add(&self, a: i64, b: i64) -> i64 { a + b }
```

A returned `McpError` becomes a tool-level error (`CallToolResult { isError:
true }`), not a transport error.

### Structured output: `Json<T>`

v3's `Json<T>` carries over. Returning `Json<T>` (with `T: Serialize +
schemars::JsonSchema`) places the value in `structuredContent`, adds a JSON text
mirror, and — new in v4 — makes the macro generate the tool's `outputSchema` from
`T`. `schemars` is re-exported as `turbomcp::schemars`.

```rust
#[derive(serde::Serialize, turbomcp::schemars::JsonSchema)]
struct Stats { count: u64 }

#[tool(description = "Stats")] async fn stats(&self) -> Json<Stats> { Json(Stats { count: 3 }) }
```

(On the `2025-11-25` wire `structuredContent` must be an object, so a `Json<T>`
serializing to a scalar/array is carried in the text mirror only there; the
`2026-07-28` wire accepts any JSON value.)

## Error constructors

`McpError::invalid_request` is gone. Use `invalid_params` for bad arguments —
the semantically correct JSON-RPC error for input validation:

```rust
// v3
return Err(McpError::invalid_request("age must be positive"));
// v4
return Err(McpError::invalid_params("age must be positive"));
```

Available constructors: `internal`, `invalid_params`, `method_not_found`,
`tool_not_found`, `tool_execution_failed`, `resource_not_found`,
`authentication`, `permission_denied`, `timeout`, `transport`.

## Contexts

v4 has a distinct context type per RPC (`CallToolContext`, `GetPromptContext`,
`ReadResourceContext`, `ListToolsContext`, …) instead of one shared
`RequestContext`. Add the relevant context as the first parameter when you need
it; omit it when you don't.

Bidirectional features (elicitation/sampling) live on `ctx.client` for the three
contexts that can carry them (`CallToolContext`, `GetPromptContext`,
`ReadResourceContext`):

```rust
#[tool(description = "Delete after confirmation")]
async fn delete(&self, ctx: &CallToolContext, path: String) -> McpResult<String> {
    let outcome = ctx.client.elicit("confirm", /* ElicitParams */).await?;
    // …
}
```

## Capabilities are derived, not declared

v3 had type-state capability builders. In v4, advertised capabilities are
*derived from the markers you write*: declaring a `#[resource]` is what
advertises the `resources` capability. There is no separate capabilities builder
to keep in sync — it cannot drift from the implementation.

## Three protocol revisions, neutral handlers

One server answers `2025-06-18`, `2025-11-25`, and the `2026-07-28` draft —
the same set v3 served, minus v3's placeholder draft string `DRAFT-2026-v1`
(which no real client sends) and plus the spec's actual one. Handlers speak
version-neutral types (`turbomcp::neutral`); the version-specific wire shapes
are conversions applied at the edges, not changes to your signatures. Most
servers never touch the wire types directly.

That includes shedding what a revision predates. Write a `#[tool(task)]` with
an icon and a title: a `2025-11-25` client sees all three, a `2025-06-18`
client sees the title and neither of the others, and `tasks/*` answers `-32601`
on that session. You write it once.

Narrow the set with `#[server(protocols("2025-11-25", "2026-07-28"))]` — worth
doing before the draft freezes, since its wire shapes can still change.

## Architecture changes (no drop-in equivalent yet)

These v3 constructs were replaced by different v4 mechanisms; code using them
needs rethinking rather than a mechanical port:

- **Middleware** — v3's `McpMiddleware` / `McpHandler` is replaced by
  `tower::Layer` composition over the `Service<JsonRpcMessage>` dispatcher (e.g.
  the telemetry `TraceContextLayer`). Cross-cutting concerns like auth and rate
  limiting are HTTP-transport-level seams (`HttpConfig::with_authenticator` /
  `with_rate_limiter`), not RPC middleware.
- **Composition** — `CompositeHandler` (mounting sub-servers under prefixes) has
  no v4 equivalent yet.
- **Visibility / progressive disclosure** — `VisibilityLayer` / `ComponentFilter`
  have no v4 equivalent yet.
- **Tags & versioning metadata** on components — not yet modeled in v4.

## Tasks

v3 surfaced Tasks one way. In v4 they split by protocol version:

- `2025-11-25`: Tasks are **core** — enable with `ServerBuilder::with_tasks()`.
  Mark individual tools taskable with `#[tool(task)]` (advertised per-tool as
  `taskSupport: optional`; unmarked tools become `forbidden`). If no tool is
  marked, every tool defaults to `optional`.
- `2026-07-28`: Tasks are an **extension** (`io.modelcontextprotocol/tasks`,
  SEP-2663) — enable the `ext-tasks` feature and register
  `TasksExtension` with `ServerBuilder::with_extension(...)`. The draft is
  session-less and server-directed (`resultType: "task"`, `tasks/get|update|cancel`,
  `notifications/tasks`).

## Not yet ported from v3

Tracked for later phases; absent in this alpha:

- **Transports**: TCP and Unix-socket — **dropped in v4**, not deferred. MCP
  defines stdio + Streamable HTTP; no MCP client speaks a raw socket, and those
  transports had no auth or Origin concept. v4 surfaces stdio, HTTP, and
  WebSocket (enable the `websocket` feature and serve with
  `turbomcp::ws::serve_websocket`). Use stdio for local IPC, HTTP/WebSocket for
  network access.

## Companion crates published for v3

These shipped alongside `turbomcp` 3.x and have **no 4.x release**. There is no
4.x crate to upgrade to, so a project using one of them should stay on the `3.x`
line for that piece — the two majors are independent crates and can coexist in
one workspace while you migrate the server/client code.

| v3 crate (3.1.5) | Status in v4 |
|---|---|
| `turbomcp-cli` | Not ported. Use the official [MCP Inspector](https://github.com/modelcontextprotocol/inspector) to poke at a server, or drive one from the [`client` example](examples/client.rs). |
| `turbomcp-proxy` | Not ported. Planned to be rebuilt on `turbomcp-client` post-GA. |
| `turbomcp-openapi` | Not ported. Planned post-GA. |
| `turbomcp-wasm` / `turbomcp-wasm-macros` | No WASM *transport* at GA, but the foundation (`turbomcp-core`, `-codec`, `-protocol`) builds `no_std` for `wasm32-unknown-unknown` — CI enforces it on every run. The bindings layer is a later effort. |
| `turbomcp-grpc` | Not ported, and unlikely to be: gRPC is not an MCP-spec transport. |
| `turbomcp-dpop` | Not ported. DPoP (RFC 9449) is still absent from the MCP spec; `Identity::Dpop` exists in v4 but has no validator. |

`turbomcp-tcp`, `turbomcp-unix`, `turbomcp-transport-streamable`, `turbomcp-wire`,
and `turbomcp-types` were internal decompositions of the v3 SDK; their roles are
covered by the v4 crates in the table at the top of the [README](README.md).
