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

## Middleware: `McpMiddleware` → `tower::Layer`

v3's `McpMiddleware` was a trait with one hook per operation (`on_call_tool`,
`on_read_resource`, `on_list_tools`, …), each returning a boxed future, assembled
into a `MiddlewareStack`. v4 has no MCP-specific middleware trait: the dispatcher
*is* a `tower::Service<JsonRpcMessage>`, so middleware is a plain
[`tower::Layer`] over it — one `call`, every method, every transport.

The practical differences:

- **The hook list can't fall behind the protocol.** v3 needed a new trait method
  for every new MCP operation, and a middleware written before `completion/complete`
  or `tasks/*` simply never saw them. A v4 layer sees every frame, including
  methods added after it was written.
- **It composes with the tower ecosystem.** `ServiceBuilder`, `timeout`,
  `ConcurrencyLimit`, `load_shed`, `retry`, and anything else written against
  `tower::Service` stack onto an MCP server unchanged.
- **No boxing is required.** v3's hooks returned `Pin<Box<dyn Future>>` per call
  by signature. A v4 layer names its future type, so a pass-through layer like
  `TracingLayer` allocates nothing.

`tower` is re-exported as `turbomcp::tower`, version-matched to the one behind
`McpService` — use it rather than a separate dependency, or the `Layer` you write
won't be the `Layer` the SDK expects.

### Porting a hook

Where v3 gave you a parsed `name: &str` / `args: Value`, v4 gives you the frame;
match on `req.method()` and read `params`. Use the `turbomcp::methods` constants
rather than string literals.

| v3 hook | v4: match `req.method()` on |
|---|---|
| `on_initialize` | `methods::request::INITIALIZE` |
| `on_list_tools` | `methods::request::TOOLS_LIST` |
| `on_call_tool` | `methods::request::TOOLS_CALL` (tool name: `params["name"]`) |
| `on_list_resources` | `methods::request::RESOURCES_LIST` |
| `on_list_resource_templates` | `methods::request::RESOURCES_TEMPLATES_LIST` |
| `on_read_resource` | `methods::request::RESOURCES_READ` (uri: `params["uri"]`) |
| `on_list_prompts` | `methods::request::PROMPTS_LIST` |
| `on_get_prompt` | `methods::request::PROMPTS_GET` |
| `on_shutdown` | not a hook — fire `ServeConfig::shutdown` (a `CancellationToken`) and the driver drains in-flight handlers |

`Next` becomes `self.inner.call(req)`; not calling it is how you short-circuit.
Two things have no v3 analogue and are worth knowing:

- A handler's `McpError` arrives as `Ok(Some(Response { error }))`, not `Err`.
  `Err(ProtocolError)` means the connection machinery failed.
- The driver clones the service stack per request, so layer state must sit behind
  an `Arc`.

Build refusals with `turbomcp::mcp_to_jsonrpc_error` so the code matches what the
dispatcher would have produced. See
[`examples/middleware.rs`](examples/middleware.rs) for an observing layer and a
short-circuiting one, end to end.

Note that **auth and rate limiting are not RPC middleware in v4** — they are
transport-level seams (`HttpConfig::with_authenticator` / `with_rate_limiter`),
because both need the HTTP request (headers, peer address) that a JSON-RPC frame
no longer carries. Per-tool authorization is `#[tool(scopes(…))]`, checked
against `ctx.base.identity`.

## Tags: `ComponentMeta` → `tags(…)` on the markers

v3 attached tags with `#[tool(tags = ["admin"])]` and read them back through
`ComponentMeta`. v4 spells it `tags("admin", "dangerous")` — the same shape as
`scopes(…)` — and accepts it on all three markers:

```rust
// v3
#[tool(description = "Wipe", tags = ["admin", "dangerous"])]
// v4
#[tool(description = "Wipe", tags("admin", "dangerous"))]
```

Read them back with `turbomcp::tags`:

```rust
use turbomcp::tags;

if tags::has(&tool.meta, "admin") { /* … */ }
tags::has_any(&tool.meta, &["experimental", "deprecated"]);
```

Two deliberate differences:

- **The `_meta` key is namespaced.** v3 wrote bare `tags` / `version` keys.
  That is spec-legal — a `_meta` prefix is optional — but unprefixed names are
  exactly where the MCP schema reserves purpose-specific metadata, so a bare
  `tags` squats on a name the spec may define. v4 writes
  `io.turbomcp/tags`. A client or proxy reading v3's key needs updating.
- **There is no per-component `version`.** v3's `ComponentMeta.version` was a
  string whose only operation was exact equality, with no consumer but the
  filter. v4 answers component evolution where clients actually look: the wire
  name (`#[tool(name = "search.v2")]`), which is decoupled from the Rust method
  name for exactly this. `_meta` stays open, so
  `.with_meta_entry("com.example/version", …)` on a hand-built component still
  works if you want one.

Tags **describe, they do not enforce** — the same was true in v3, but it is
worth restating: a tag hides nothing and permits nothing by itself.
`#[tool(scopes("admin"))]` is the authorization mechanism, checked against
`ctx.base.identity`.

## Composition: `CompositeHandler` → `Composite`

v3's `CompositeHandler::new(name, version).mount(handler, "weather")` becomes:

```rust
use turbomcp::{Composite, Implementation};

let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
    .mount("weather", Weather.into_server())?
    .mount("news", News.into_server())?
    .into_server()
    .build();
```

`mount` takes the prefix first and a `ServerBuilder` (what `into_server()`
produces) rather than a bare handler, because that is where a server's
capabilities are registered — the composite advertises exactly what its mounts
between them provide, so capabilities stay derived rather than declared.

Namespacing differs in one place, deliberately:

| | v3 | v4 |
|---|---|---|
| Tools | `{prefix}_{name}` | `{prefix}.{name}` |
| Prompts | `{prefix}_{name}` | `{prefix}.{name}` |
| Resources | `{prefix}://{uri}` | **unchanged** |

- **`.` rather than `_`.** `_` is common *inside* tool names, so
  `weather_get_forecast` is ambiguous about where the prefix ends. `.` is in the
  spec's name charset and already reads as a namespace. A mount prefix may
  therefore not contain `.`, which is checked at `mount`.
- **Resource URIs are no longer rewritten.** v3's rule produced
  `weather://config://app` for a resource at `config://app` — and more
  fundamentally, a URI is a global identifier a client may hand elsewhere, so
  rewriting it makes it a lie. Two mounts claiming one URI is a genuine
  ambiguity and `resources/list` reports it, the same way the `#[server]` macro
  rejects a duplicate URI within one server. Give each mounted server its own
  scheme or authority.

Two things `mount` rejects rather than silently ignoring:

- **Dispatcher-level settings on a mounted builder** (`with_tasks`,
  `with_session_backend`, `cache_policy`, …). There is one dispatcher and it is
  the composite's; configure it there.
- **A mount that accepts fewer protocol revisions than the composite.**
  Handlers are version-neutral, so a sub-server's `protocols(…)` pin cannot be
  honored once it is mounted — the composite's dispatcher owns wire rendering.
  Narrow the composite with `Composite::protocols(…)` instead.

## Visibility: `VisibilityLayer` → `with_visibility`

v3's `VisibilityLayer::new(handler).with_disabled(ComponentFilter::with_tags(["admin"]))`
becomes a builder setting:

```rust
use std::sync::Arc;
use turbomcp::Visibility;

let dispatcher = MyServer
    .into_server()
    .with_visibility(Arc::new(
        Visibility::new()
            .hiding_tagged(["internal"])
            .requiring_declared_scopes(),
    ))
    .build();
```

Three differences worth knowing:

- **Hidden means unreachable.** v3 filtered lists. v4 also refuses the call —
  and refuses it *exactly as a component that does not exist* (unknown tool,
  unknown prompt, resource-not-found), because a distinct "forbidden" answer
  would disclose the existence the policy is hiding. This costs one list per
  guarded call, paid only when a policy is installed. A hidden *template's*
  URIs are refused too, since a template's URIs are not enumerable and checking
  only the concrete list would leave the whole template readable.
- **`requiring_declared_scopes()` closes a real gap.** `#[tool(scopes("admin"))]`
  has always refused the call, but the tool still appeared in `tools/list` for a
  caller who could never use it. Turning this on makes the catalog agree with
  the enforcement. The scopes a tool declares are published in its `_meta`
  (`io.turbomcp/scopes`) so the filter can read them at list time — and so they
  survive being mounted into a `Composite`.
- **There is no session map.** v3's `VisibilityLayer` owned a per-session
  enable/disable map, and its own documentation warned the map leaks without
  explicit cleanup. A v4 policy is a function of `(component, request)` — and
  `VisibleComponent::request` is the whole `RequestContext`, identity included —
  so a deployment that wants per-session unlocking implements `VisibilityPolicy`
  over its own store and owns that store's lifecycle. A bare closure is a
  policy:

  ```rust
  let policy = Arc::new(|c: &VisibleComponent<'_>| {
      c.id != "rotate_keys" || unlocked_for(c.request.identity.subject())
  });
  ```

`ComponentFilter`'s include/exclude/version triple has no direct counterpart:
`hiding_tagged` covers exclusion, inclusion is the complement of it, and there
is no per-component version to filter on (see the tags section above).

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
