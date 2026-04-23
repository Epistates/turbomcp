# TurboMCP Macros

[![Crates.io](https://img.shields.io/crates/v/turbomcp-macros.svg)](https://crates.io/crates/turbomcp-macros)
[![Documentation](https://docs.rs/turbomcp-macros/badge.svg)](https://docs.rs/turbomcp-macros)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Procedural macros for MCP server development with automatic schema generation.**

## Table of Contents

- [Overview](#overview)
- [Exported Macros](#exported-macros)
- [`#[server]`](#server)
- [`#[tool]`](#tool)
- [`#[resource]`](#resource)
- [`#[prompt]`](#prompt)
- [`#[description]`](#description)
- [How schemas are generated](#how-schemas-are-generated)
- [Context injection](#context-injection)
- [Feature flags](#feature-flags)
- [Development](#development)

## Overview

`turbomcp-macros` provides the procedural macros for TurboMCP development. The macros
discover handler methods inside an `impl` block, parse their signatures, and generate a
full `McpHandler` implementation plus JSON schemas for the tool inputs.

Schema generation uses `schemars` and is always enabled — it is not an optional feature.

## Exported Macros

The crate exports exactly these five attribute macros:

| Macro | Role |
|---|---|
| `#[server]` | Transforms an `impl` block into a full `McpHandler` implementation |
| `#[tool]` | Marks a method as a tool handler (must be inside a `#[server]` block) |
| `#[resource]` | Marks a method as a resource handler (must be inside a `#[server]` block) |
| `#[prompt]` | Marks a method as a prompt handler (must be inside a `#[server]` block) |
| `#[description]` | Attaches a description string to a tool parameter for JSON Schema |

No other macros are provided by this crate. Used outside a `#[server]` block, the
handler attributes emit a compile error with a usage example.

## `#[server]`

Applies to an inherent (non-trait) `impl` block. Generates an `impl McpHandler` for
the struct that dispatches tool / resource / prompt calls to the annotated methods.

**Supported arguments:**

- `name = "..."` — Server name. Defaults to the struct identifier.
- `version = "..."` — Server version. Defaults to `"1.0.0"`.
- `description = "..."` — Optional server description.

The deprecated `transports = [...]` argument is rejected with a diagnostic directing
you to Cargo feature flags instead.

The macro scans impl methods for exactly three attribute names — `tool`, `resource`,
`prompt` — and passes everything else through unchanged.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(name = "calculator", version = "1.0.0")]
impl Calculator {
    /// Add two numbers
    #[tool]
    async fn add(&self, a: f64, b: f64) -> f64 {
        a + b
    }
}
```

The runner methods (`run_stdio`, `run_http`, `run_tcp`, `run_unix`, `run_websocket`)
are defined on the `McpServer` trait in `turbomcp-server`, which is blanket-implemented
for every type that has an `McpHandler`. They are *not* generated per-call by this
macro; the macro's job is only to produce the `McpHandler` impl.

## `#[tool]`

Marks a method as a tool handler. The tool name is the method identifier, and the
tool description is taken from doc comments unless overridden via the attribute.

**Supported argument forms:**

- `#[tool]` — no arguments; description comes from the `///` doc comment.
- `#[tool("description")]` — shorthand for the description.
- `#[tool(description = "...", tags = ["a", "b"], version = "1.0")]` — named arguments.

Recognized named keys: `description`, `tags`, `version`. Unknown keys are silently
ignored by the parser.

```rust
#[server]
impl MyServer {
    /// Greet someone by name
    #[tool]
    async fn greet(
        &self,
        #[description("Name of the person to greet")] name: String,
        #[description("Optional greeting prefix")] prefix: Option<String>,
    ) -> String {
        let prefix = prefix.unwrap_or_else(|| "Hello".into());
        format!("{prefix}, {name}!")
    }
}
```

**Input schema rules:**

- `&self` is skipped.
- Parameters whose type is `Context`, `RequestContext`, `&Context`, or `&RequestContext`
  are recognized as context and excluded from the schema.
- Remaining parameters become schema properties; `Option<T>` parameters are optional,
  everything else is required.
- Parameter types are passed through `schemars::schema_for!` at compile time.

## `#[resource]`

Marks a method as a resource handler. Requires a URI template as the first argument.

**Supported argument forms:**

- `#[resource("uri://template")]`
- `#[resource("uri://template", mime_type = "application/json")]`
- `#[resource("uri://template", tags = ["..."], version = "1.0")]`

```rust
#[server]
impl MyServer {
    /// Application configuration
    #[resource("config://app", mime_type = "application/json")]
    async fn config(&self, uri: String, ctx: &RequestContext) -> String {
        r#"{"debug": false}"#.to_string()
    }

    /// Read a file by path
    #[resource("file://{path}")]
    async fn file(&self, uri: String, ctx: &RequestContext) -> String {
        format!("Content of {}", uri)
    }
}
```

## `#[prompt]`

Marks a method as a prompt handler. Arguments are optional; a bare `#[prompt]` uses
the method's doc comment as the prompt description.

```rust
#[server]
impl MyServer {
    /// Generate a greeting prompt
    #[prompt]
    async fn greeting(&self, name: String, ctx: &RequestContext) -> String {
        format!("Hello {}! How can I help you today?", name)
    }
}
```

Function parameters (other than `&self` and context) are exposed as prompt arguments.

## `#[description]`

Attaches a description string to a tool parameter. Both forms are accepted:

- `#[description("text")]`
- `#[description = "text"]`

The string is embedded into the JSON Schema `description` for that property.

```rust
#[tool]
async fn search(
    &self,
    #[description("The search query string")] query: String,
    #[description("Maximum number of results")] limit: Option<u32>,
) -> Vec<String> {
    // ...
}
```

## How schemas are generated

Each non-context parameter is run through `schemars::schema_for!(T)` at macro
expansion time. The resulting schema is merged into an object schema with:

- `type: "object"`
- `properties`: one entry per parameter, with any `#[description(...)]` string
  merged in
- `required`: names of non-`Option` parameters
- `additionalProperties: false`

Complex user-defined types work automatically as long as they implement
`schemars::JsonSchema` (usually via `#[derive(JsonSchema)]`).

## Context injection

Any parameter whose type resolves to `Context` or `RequestContext` (owned or `&`)
is recognized as the request context. It is excluded from the generated schema and
wired from the dispatcher at call time. The context parameter may appear in any
position in the signature.

## Feature flags

The macro crate itself has a small set of features that gate optional dependencies
needed by generated code that uses certain transports:

| Feature | Enables |
|---|---|
| `http` | pulls in `axum` for HTTP-related code paths |
| `tcp` | pulls in `tokio` and `turbomcp-transport` |
| `unix` | pulls in `tokio` and `turbomcp-transport` |
| `experimental-tasks` | pass-through to `turbomcp-protocol/experimental-tasks` |

In practice you enable the matching features on the umbrella `turbomcp` crate and
the dependency graph resolves these transitively. `schemars` is always required.

## Development

```bash
# Build / test
cargo build -p turbomcp-macros
cargo test  -p turbomcp-macros

# Inspect expanded macro output
cargo expand --package your-server-crate
```

## Related Crates

- **[turbomcp](../turbomcp/)** — Main SDK (re-exports these macros via `prelude`)
- **[turbomcp-server](../turbomcp-server/)** — Provides the `McpServer` trait and
  `run_stdio` / `run_http` / etc.
- **[turbomcp-protocol](../turbomcp-protocol/)** — Protocol types and message definitions

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) Rust SDK for the Model Context Protocol.*
