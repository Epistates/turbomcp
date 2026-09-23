# Dependency Injection

How TurboMCP gets values into a handler, and why it has no dependency-injection
container.

For day-to-day usage, see [Context & Shared State](../guide/context-injection.md).
This page covers the design and what the `#[server]` macro generates.

## Overview

TurboMCP has no dependency-injection container, provider registry, or
`TypeId` lookup. A handler gets everything it uses from two places:

1. **Request parameters** — deserialized from the JSON-RPC request's
   `arguments`. Their types make up the tool's input schema.
2. **`&RequestContext`** — the only injected parameter. Any parameter whose type
   is `RequestContext` or `Context` (owned or by reference) receives the
   request's context and is left out of the schema.

Everything else a handler needs — configuration, database pools, caches, HTTP
clients — lives on the server struct, which `#[server]` requires to be `Clone`
and which every transport clones. Keep it cheap to clone by holding shared
services behind an `Arc`.

```rust
use std::sync::Arc;
use turbomcp::prelude::*;

/// Stands in for a database pool, HTTP client, ...
pub struct Directory {
    names: Vec<String>,
}

#[derive(Clone)]
pub struct UserServer {
    directory: Arc<Directory>,
}

#[server(name = "users", version = "1.0.0")]
impl UserServer {
    /// Look a user up by index.
    #[tool]
    async fn get_user(
        &self,
        // Request parameter: from `arguments`, part of the input schema
        index: usize,
        // Injected: not in the schema
        ctx: &RequestContext,
    ) -> McpResult<String> {
        let name = self
            .directory
            .names
            .get(index)
            .ok_or_else(|| McpError::invalid_params(format!("no user at {index}")))?;
        Ok(format!("{name} (request {})", ctx.request_id()))
    }
}
```

## Why No Container

- **Types are checked by the compiler.** A missing dependency is a missing
  struct field, not a runtime lookup failure.
- **Nothing to register or resolve per request.** Dispatch is a `match` on the
  tool name followed by argument extraction.
- **The same code runs on WASM.** `McpHandler` is `no_std`-friendly and has no
  `Send`/`Any` requirements beyond `MaybeSend`/`MaybeSync`.
- **Tests construct the server directly**, with whatever implementations they
  need (see [Testing](#testing)).

## What the Macro Generates

For each `#[tool]`, `#[server]` generates a `tools/call` arm on the generated
`McpHandler::call_tool`. In outline — a sketch, not the literal expansion:

```rust,ignore
fn call_tool<'a>(&'a self, name: &'a str, args: Value, ctx: &'a RequestContext)
    -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a
{
    async move {
        match name {
            "get_user" => {
                // 1. Reject arguments the tool does not declare
                //    (the schema says `additionalProperties: false`)
                // 2. Deserialize each declared parameter; a missing, mistyped,
                //    or oversized argument is `invalid_params`
                let index: usize = /* from args["index"] */;
                // 3. Call the method, passing `ctx` where the signature asks for it
                let result = self.get_user(index, ctx).await;
                // 4. Convert with `IntoToolResult`. An `McpError` becomes a tool
                //    execution error (`isError: true`) with its kind in `_meta`.
                convert(result)
            }
            _ => Err(McpError::tool_not_found(name)),
        }
    }
}
```

Resources and prompts follow the same idea with fixed shapes: a resource
handler takes `(uri: String, ctx: &RequestContext)`, and a prompt handler takes
its arguments (`String` or `Option<String>`) followed by `ctx`.

### Parameter Detection

1. `self` is skipped.
2. A parameter whose type's last path segment is `RequestContext` or `Context`
   (behind `&` or not) is the context.
3. Every other parameter is a request parameter. `Option<T>` is optional; any
   other type is required. Its schema comes from `schemars`, so a custom type
   needs `#[derive(Deserialize, schemars::JsonSchema)]`.

## Request-Scoped Data

Per-request values travel on the context rather than through injected types:

| Need | Where it is |
|---|---|
| Request ID, transport | `ctx.request_id()`, `ctx.transport()` |
| Session | `ctx.session_id()` (Streamable HTTP) |
| Authenticated identity | `ctx.principal()`, `ctx.subject()`, `ctx.has_any_role(..)` |
| HTTP headers | `ctx.header("user-agent")` |
| Cancellation, progress | `ctx.is_cancelled()`, `ctx.report_progress(..)` |
| Server-to-client requests | `ctx.sample(..)`, `ctx.elicit_form(..)`, `ctx.list_roots()` |
| Per-session key/value state | `turbomcp_protocol::RichContextExt` (`ctx.get_state`, `ctx.set_state`) |

## Testing

Because dependencies are ordinary fields, a test substitutes them by building
the server with a different value. Make the server generic over a trait, or
hold a trait object, where a test needs a fake:

```rust
use std::sync::Arc;
use turbomcp::prelude::*;

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> String;
}

#[derive(Clone)]
pub struct TimeServer {
    clock: Arc<dyn Clock>,
}

#[server(name = "time", version = "1.0.0")]
impl TimeServer {
    /// The current time.
    #[tool]
    async fn now(&self) -> String {
        self.clock.now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> String {
            "2025-01-01T00:00:00Z".to_string()
        }
    }

    #[tokio::test]
    async fn reports_the_clock() {
        let client = McpTestClient::new(TimeServer { clock: Arc::new(FixedClock) });
        let result = client.call_tool("now", serde_json::json!({})).await.unwrap();
        assert_eq!(result.first_text(), Some("2025-01-01T00:00:00Z"));
    }
}
```

## Related Documentation

- [Context & Shared State](../guide/context-injection.md) - Using the request context
- [System Design](./system-design.md) - Architecture overview
- [Context Lifecycle](./context-lifecycle.md) - Request flow
- [Protocol Compliance](./protocol-compliance.md) - MCP protocol
- [Advanced Patterns](../guide/advanced-patterns.md) - Handler patterns
