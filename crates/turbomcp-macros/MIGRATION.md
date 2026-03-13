# turbomcp-macros Migration Guide

For workspace-level migration (error types, transport architecture, crate structure), see the
[top-level MIGRATION.md](../../MIGRATION.md).

This document covers only changes to the procedural macros in `turbomcp-macros`.

---

## v2.x to v3.0

### Macros that exist in v3.0

The complete set of exported macros in v3.0.2 is:

| Macro | Role |
|---|---|
| `#[server]` | Transforms an `impl` block into a full `McpHandler` implementation |
| `#[tool]` | Marks a method as a tool handler (must be inside a `#[server]` block) |
| `#[resource]` | Marks a method as a resource handler (must be inside a `#[server]` block) |
| `#[prompt]` | Marks a method as a prompt handler (must be inside a `#[server]` block) |
| `#[description]` | Attaches a description string to a tool parameter for JSON Schema generation |

No other macros are exported. `#[elicitation]`, `#[completion]`, `#[template]`, `#[ping]`,
`elicit!`, and `mcp_error!` do not exist in this crate at any version.

### Breaking changes

**`#[server]` now resolves the crate path dynamically.**
The generated code resolves the host crate as `turbomcp` or `turbomcp-server` (falling back to
`crate`). If your v2.x server was built against internal crate paths directly, the generated trait
impl may reference a different path. Using `turbomcp::prelude::*` is the supported import style.

**Transport method generation is feature-gated.**
The `#[server]` macro generates transport runner methods (`run_stdio`, `run_http`, etc.) only when
the corresponding feature is enabled in `turbomcp-macros`. In v3.0 these features are:

| Feature | Generated method |
|---|---|
| _(default, no feature)_ | `run()` via STDIO |
| `http` | HTTP/SSE transport methods (requires `axum`) |
| `tcp` | TCP transport methods (requires `tokio`, `turbomcp-transport`) |
| `unix` | Unix socket transport methods (requires `tokio`, `turbomcp-transport`) |

In v2.x the transport feature gating was handled differently. If you were relying on specific
generated method names, verify them after upgrading with `cargo expand`.

**`analyze_impl()` only processes `#[tool]`, `#[resource]`, and `#[prompt]`.**
The `#[server]` macro scans for exactly these three attribute names on methods in the impl block.
Any other custom attribute macros attached to methods in the block are passed through unchanged and
are not treated as MCP handlers.

**Error types in generated code changed.**
Generated handler dispatch code uses `McpError` / `McpResult` from the unified error type
introduced in v3.0. See the top-level migration guide for the full error type change.

### No-change items

The calling syntax for all four handler macros is unchanged from v2.x:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Compute the sum of two integers
    #[tool]
    async fn add(
        &self,
        #[description("First operand")] a: i64,
        #[description("Second operand")] b: i64,
    ) -> i64 {
        a + b
    }

    /// Application configuration
    #[resource("config://app", mime_type = "application/json")]
    async fn config(&self, uri: String, ctx: &RequestContext) -> String {
        r#"{"debug": false}"#.to_string()
    }

    /// Greeting prompt
    #[prompt]
    async fn greet(&self, name: String, ctx: &RequestContext) -> String {
        format!("Hello, {}! How can I help you today?", name)
    }
}
```

Schema generation via `schemars` is always enabled and is not an optional feature.

---

## v1.x to v2.0

### Macros that existed in v1.x and v2.x

The same four handler macros existed in v1.x: `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]`.
The `#[description]` parameter attribute was added during the v2.x line.

### Breaking changes

**`#[description]` on parameters is a v2.x addition.**
In v1.x, parameter descriptions could only come from doc comments (`///`) on the function. The
`#[description("...")]` attribute on individual parameters was not available. Code using
`#[description]` will not compile against v1.x of this crate.

**`#[resource]` gained a `mime_type` argument.**
The `mime_type = "..."` named argument on `#[resource]` was not present in v1.x. Existing
`#[resource("uri")]` usage without `mime_type` continues to work.

**`#[tool]` accepts an optional description string.**
`#[tool("Custom description")]` as a shorthand for the tool description was introduced during
v2.x. In v1.x only doc comments were used for tool descriptions.

### Updating dependencies

```toml
# v1.x
[dependencies]
turbomcp-macros = "1.x"

# v2.x (use via the main crate — direct dependency on turbomcp-macros is not required)
[dependencies]
turbomcp = "2.x"
```

`turbomcp-macros` is re-exported through the `turbomcp` crate. Direct dependence on
`turbomcp-macros` is only necessary when building a server crate that does not use the
`turbomcp` umbrella crate.

---

## Resources

- Top-level migration guide (error types, transports, crate structure): [`../../MIGRATION.md`](../../MIGRATION.md)
- Macro API documentation: <https://docs.rs/turbomcp-macros>
- Issue tracker: <https://github.com/Epistates/turbomcp/issues>
