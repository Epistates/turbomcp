# TurboMCP v3 Unified Architecture - Deep Research & Design

> **Status**: Design Document
> **Date**: 2026-01-15
> **Goal**: Pristine, SOTA, unified architecture for native + WASM

## Executive Summary

After deep research into Rust async patterns, the official MCP SDK, embedded-hal design patterns, and cross-platform WASM/native challenges, this document presents a comprehensive v3 architecture that achieves:

1. **True Unified Handler Trait** - Single `McpHandler` definition in `turbomcp-core`
2. **Platform-Adaptive Bounds** - Conditional `Send` bounds via marker trait
3. **Zero-Boilerplate DX** - Macro generates platform-appropriate code
4. **no_std Foundation** - Core types work everywhere (embedded, WASM, native)
5. **Security-First Design** - Input validation, error sanitization built-in

## Research Findings

### 1. The Send/Sync Problem

The fundamental challenge for unified native/WASM code is [`JsValue` being `!Send`](https://github.com/rustwasm/wasm-bindgen/issues/2753):

- **Native**: Multi-threaded, requires `Send + Sync` for futures
- **WASM**: Single-threaded, `JsValue` is `!Send` by design (slab-based object management)

This has "huge trickle down effects" on downstream crates. The solution is **conditional compilation** with a marker trait.

### 2. Official MCP SDK Architecture

The [official Rust MCP SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk) uses:

```rust
pub trait Service<R: ServiceRole>: Send + Sync + 'static {
    fn handle_request(&self, request: R::PeerReq, context: RequestContext<R>)
        -> impl Future<Output = Result<R::Resp, McpError>> + Send + '_;
}
```

**Limitations**:
- Tokio-dependent (`tokio_util::sync::CancellationToken`)
- Not `no_std` compatible
- No WASM support

### 3. Embedded-HAL Pattern

The [embedded-hal ecosystem](https://github.com/rust-embedded/embedded-hal) demonstrates the gold standard for hardware abstraction:

- **embedded-hal**: Sync traits (`no_std`)
- **embedded-hal-async**: Async traits (requires Rust 1.75+)
- **Minimal API**: "Easy to implement and zero cost, yet highly composable"

Key insight: Separate sync metadata from async operations.

### 4. Tower/Axum Service Pattern

[Axum's integration with Tower](https://leapcell.io/blog/unpacking-the-tower-abstraction-layer-in-axum-and-tonic) shows:

- `Service` trait defines single unit of work
- `Layer` enables composition and modification
- Transport-agnostic design

### 5. Cross-Platform Async Patterns

From [Rust forum discussions](https://users.rust-lang.org/t/code-patterns-for-working-with-async-in-cross-platform-wasm-native-context/130558):

- Use `#[async_trait(?Send)]` for WASM compatibility
- Define traits with conditional Send bounds
- Runtime abstraction via feature flags

## Architecture Design

### Layer Diagram

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        Application Layer                                  │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                   │
│   │   Your MCP   │  │  Cloudflare  │  │   Examples   │                   │
│   │   Server     │  │   Worker     │  │              │                   │
│   └──────────────┘  └──────────────┘  └──────────────┘                   │
└──────────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                        Facade Layer (turbomcp)                            │
│   • Re-exports based on target/features                                   │
│   • Unified prelude for all platforms                                     │
└──────────────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ turbomcp-server │  │  turbomcp-wasm  │  │turbomcp-macros  │
│   (std, tokio)  │  │ (wasm-bindgen)  │  │  (proc-macro)   │
│                 │  │                 │  │                 │
│ • McpHandlerExt │  │ • WasmServer    │  │ • #[server]     │
│ • run_stdio()   │  │ • run_http()    │  │ • #[tool]       │
│ • run_http()    │  │ • Worker integ  │  │ • #[resource]   │
│ • run_tcp()     │  │                 │  │ • #[prompt]     │
└─────────────────┘  └─────────────────┘  └─────────────────┘
              │               │               │
              └───────────────┼───────────────┘
                              ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                     Core Layer (turbomcp-core)                            │
│   • McpHandler trait (unified, platform-adaptive Send bounds)             │
│   • RequestContext (minimal, no_std compatible)                           │
│   • MaybeSend marker trait                                                │
│   • BoxFuture type aliases                                                │
│   no_std compatible with alloc                                            │
└──────────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                     Types Layer (turbomcp-types)                          │
│   • ALL MCP type definitions (Tool, Resource, Prompt, ServerInfo)         │
│   • Result types (ToolResult, ResourceResult, PromptResult)               │
│   • Error types (McpError with JSON-RPC codes)                            │
│   • Conversion traits (IntoToolResult, IntoResourceResult, etc.)          │
│   Single source of truth - no_std compatible                              │
└──────────────────────────────────────────────────────────────────────────┘
```

### The Unified Handler Trait

The key innovation is a **conditional Send bound** via marker trait:

```rust
// turbomcp-core/src/marker.rs

/// Marker trait that's `Send` on native, nothing on WASM.
/// This enables unified trait definitions that work on both platforms.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}

#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// Marker trait that's `Sync` on native, nothing on WASM.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync> MaybeSync for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}

#[cfg(target_arch = "wasm32")]
impl<T> MaybeSync for T {}
```

```rust
// turbomcp-core/src/handler.rs

use core::future::Future;
use alloc::vec::Vec;
use serde_json::Value;
use turbomcp_types::{
    McpResult, Prompt, PromptResult, Resource, ResourceResult,
    ServerInfo, Tool, ToolResult,
};
use crate::{MaybeSend, MaybeSync, RequestContext};

/// The unified MCP handler trait.
///
/// This trait defines the complete interface for an MCP server. It's designed to:
/// - Work identically on native (std) and WASM (no_std) targets
/// - Be automatically implemented by the `#[server]` macro
/// - Enable zero-boilerplate server development
///
/// # Platform Behavior
///
/// - **Native**: Methods return `impl Future + Send`, enabling multi-threaded executors
/// - **WASM**: Methods return `impl Future`, compatible with single-threaded runtimes
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool]
///     async fn greet(&self, name: String) -> String {
///         format!("Hello, {}!", name)
///     }
/// }
/// ```
pub trait McpHandler: Clone + MaybeSend + MaybeSync + 'static {
    /// Returns server metadata for the initialize handshake.
    fn server_info(&self) -> ServerInfo;

    /// Returns all available tools.
    fn list_tools(&self) -> Vec<Tool>;

    /// Returns all available resources.
    fn list_resources(&self) -> Vec<Resource>;

    /// Returns all available prompts.
    fn list_prompts(&self) -> Vec<Prompt>;

    /// Calls a tool by name with the given arguments.
    ///
    /// # Arguments
    /// * `name` - The tool name to call
    /// * `args` - JSON arguments for the tool
    /// * `ctx` - Request context with transport info
    ///
    /// # Errors
    /// Returns `McpError::tool_not_found` if the tool doesn't exist.
    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a;

    /// Reads a resource by URI.
    ///
    /// # Arguments
    /// * `uri` - The resource URI to read
    /// * `ctx` - Request context with transport info
    ///
    /// # Errors
    /// Returns `McpError::resource_not_found` if the resource doesn't exist.
    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a;

    /// Gets a prompt by name with optional arguments.
    ///
    /// # Arguments
    /// * `name` - The prompt name to get
    /// * `args` - Optional JSON arguments for the prompt
    /// * `ctx` - Request context with transport info
    ///
    /// # Errors
    /// Returns `McpError::prompt_not_found` if the prompt doesn't exist.
    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a;
}
```

### Minimal Request Context

```rust
// turbomcp-core/src/context.rs

use alloc::string::String;
use alloc::collections::BTreeMap;

/// Transport type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransportType {
    /// Standard I/O transport
    Stdio,
    /// HTTP transport (REST or SSE)
    Http,
    /// WebSocket transport
    WebSocket,
    /// Raw TCP transport
    Tcp,
    /// Unix domain socket transport
    Unix,
    /// WebAssembly/Worker transport
    Wasm,
}

/// Minimal request context that works on all platforms.
///
/// This struct contains only the essential information needed to process
/// a request. Platform-specific extensions (cancellation tokens, UUIDs, etc.)
/// are provided by the runtime layer.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Unique request identifier (JSON-RPC id as string)
    pub request_id: String,
    /// Transport type that received this request
    pub transport: TransportType,
    /// Optional metadata (headers, user info, etc.)
    pub metadata: BTreeMap<String, String>,
}

impl RequestContext {
    /// Create a new request context.
    pub fn new(request_id: impl Into<String>, transport: TransportType) -> Self {
        Self {
            request_id: request_id.into(),
            transport,
            metadata: BTreeMap::new(),
        }
    }

    /// Add metadata to the context.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }
}
```

### Macro Code Generation

The `#[server]` macro generates platform-appropriate implementations:

```rust
// User writes (same code for native AND WASM):
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(name = "calculator", version = "1.0.0")]
impl Calculator {
    /// Add two numbers
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }

    /// Greet someone
    #[tool]
    async fn greet(&self, name: String) -> String {
        format!("Hello, {}!", name)
    }
}

// Entry point differs by platform:
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() {
    Calculator.run_stdio().await.unwrap();
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // WASM entry handled by worker runtime
}
```

**Generated code (native)**:
```rust
impl McpHandler for Calculator {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("calculator", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            Tool::new("add", "Add two numbers")
                .with_schema(json!({
                    "type": "object",
                    "properties": {
                        "a": {"type": "integer"},
                        "b": {"type": "integer"}
                    },
                    "required": ["a", "b"]
                })),
            Tool::new("greet", "Greet someone")
                .with_schema(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"}
                    },
                    "required": ["name"]
                })),
        ]
    }

    fn list_resources(&self) -> Vec<Resource> { vec![] }
    fn list_prompts(&self) -> Vec<Prompt> { vec![] }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send + 'a {
        async move {
            match name {
                "add" => {
                    let a: i64 = args.get("a").and_then(|v| v.as_i64())
                        .ok_or_else(|| McpError::invalid_params("missing 'a'"))?;
                    let b: i64 = args.get("b").and_then(|v| v.as_i64())
                        .ok_or_else(|| McpError::invalid_params("missing 'b'"))?;
                    let result = self.add(a, b).await;
                    Ok(ToolResult::text(result.to_string()))
                }
                "greet" => {
                    let name: String = args.get("name").and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| McpError::invalid_params("missing 'name'"))?;
                    let result = self.greet(name).await;
                    Ok(ToolResult::text(result))
                }
                _ => Err(McpError::tool_not_found(name))
            }
        }
    }

    fn read_resource<'a>(&'a self, uri: &'a str, _ctx: &'a RequestContext)
        -> impl Future<Output = McpResult<ResourceResult>> + Send + 'a {
        async move { Err(McpError::resource_not_found(uri)) }
    }

    fn get_prompt<'a>(&'a self, name: &'a str, _args: Option<Value>, _ctx: &'a RequestContext)
        -> impl Future<Output = McpResult<PromptResult>> + Send + 'a {
        async move { Err(McpError::prompt_not_found(name)) }
    }
}

// Extension trait for native runtime
impl McpHandlerExt for Calculator {}
```

### Runtime Extensions

**Native (`turbomcp-server`)**:
```rust
/// Extension trait providing transport runners for native targets.
pub trait McpHandlerExt: McpHandler {
    /// Run the server on STDIO transport.
    fn run_stdio(&self) -> impl Future<Output = McpResult<()>> + Send {
        stdio::run(self.clone())
    }

    /// Run the server on HTTP transport.
    fn run_http(&self, addr: SocketAddr) -> impl Future<Output = McpResult<()>> + Send {
        http::run(self.clone(), addr)
    }

    /// Run the server on WebSocket transport.
    fn run_websocket(&self, addr: SocketAddr) -> impl Future<Output = McpResult<()>> + Send {
        websocket::run(self.clone(), addr)
    }

    /// Run the server on TCP transport.
    fn run_tcp(&self, addr: SocketAddr) -> impl Future<Output = McpResult<()>> + Send {
        tcp::run(self.clone(), addr)
    }
}
```

**WASM (`turbomcp-wasm`)**:
```rust
/// Extension trait providing WASM-specific runners.
pub trait McpHandlerWasm: McpHandler {
    /// Convert to a Cloudflare Worker-compatible server.
    fn into_worker_server(self) -> WorkerMcpServer<Self> {
        WorkerMcpServer::new(self)
    }

    /// Handle a Worker request directly.
    fn handle_request(&self, req: worker::Request) -> impl Future<Output = worker::Result<worker::Response>> {
        worker::handle(self.clone(), req)
    }
}
```

## Security Design

### Input Validation (Per [Rust Security Best Practices 2025](https://corgea.com/Learn/rust-security-best-practices-2025))

```rust
// turbomcp-core/src/validation.rs

/// Validated string that's been checked for length and content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedString(String);

impl ValidatedString {
    /// Maximum allowed string length (prevents DoS).
    pub const MAX_LENGTH: usize = 65536;

    /// Create a validated string, checking length and content.
    pub fn new(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        if s.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                actual: s.len(),
                max: Self::MAX_LENGTH
            });
        }
        // Additional validation: no null bytes, valid UTF-8 (already guaranteed)
        if s.contains('\0') {
            return Err(ValidationError::InvalidContent("null byte"));
        }
        Ok(Self(s))
    }
}

/// Validated tool name (alphanumeric, underscores, hyphens only).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolName(String);

impl ToolName {
    pub fn new(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        if s.is_empty() || s.len() > 256 {
            return Err(ValidationError::InvalidLength);
        }
        if !s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(ValidationError::InvalidContent("tool name must be alphanumeric"));
        }
        Ok(Self(s))
    }
}
```

### Error Sanitization

```rust
// Never expose internal details in error messages
impl McpError {
    /// Create a safe error that doesn't leak internal details.
    pub fn safe_internal(internal_msg: &str) -> Self {
        // Log the full error internally
        tracing::error!(internal = %internal_msg, "Internal error occurred");
        // Return sanitized error to client
        Self::internal("An internal error occurred")
    }
}
```

## File Structure

```
crates/
├── turbomcp-types/          # All MCP type definitions (no_std)
│   └── src/
│       ├── lib.rs
│       ├── content.rs       # Content types
│       ├── definitions.rs   # Tool, Resource, Prompt, ServerInfo
│       ├── results.rs       # ToolResult, ResourceResult, PromptResult
│       ├── error.rs         # McpError
│       └── traits.rs        # IntoToolResult, etc.
│
├── turbomcp-core/           # Unified handler trait (no_std)
│   └── src/
│       ├── lib.rs
│       ├── marker.rs        # MaybeSend, MaybeSync
│       ├── handler.rs       # McpHandler trait
│       ├── context.rs       # RequestContext
│       ├── validation.rs    # Input validation types
│       └── response.rs      # IntoToolResponse helpers
│
├── turbomcp-macros/         # Procedural macros
│   └── src/
│       ├── lib.rs
│       └── v3/
│           ├── mod.rs
│           ├── server.rs    # #[server] macro
│           ├── tool.rs      # Tool parsing & schema gen
│           ├── resource.rs  # Resource parsing
│           ├── prompt.rs    # Prompt parsing
│           └── schema.rs    # JSON Schema generation
│
├── turbomcp-server/         # Native runtime (std, tokio)
│   └── src/
│       ├── lib.rs
│       └── v3/
│           ├── mod.rs
│           ├── ext.rs       # McpHandlerExt trait
│           ├── router.rs    # JSON-RPC routing
│           ├── stdio.rs     # STDIO transport
│           ├── http.rs      # HTTP/SSE transport
│           ├── websocket.rs # WebSocket transport
│           └── tcp.rs       # TCP transport
│
├── turbomcp-wasm/           # WASM runtime (wasm-bindgen)
│   └── src/
│       ├── lib.rs
│       ├── ext.rs           # McpHandlerWasm trait
│       ├── worker.rs        # Cloudflare Workers integration
│       └── router.rs        # WASM-compatible routing
│
└── turbomcp/                # Facade crate
    └── src/
        └── lib.rs           # Re-exports everything
```

## Migration Path

### From Current v3 to Unified v3

1. **Move `McpHandler` trait to `turbomcp-core`**
   - Add `MaybeSend`/`MaybeSync` marker traits
   - Make trait work on both platforms

2. **Simplify `RequestContext`**
   - Remove tokio-specific types (CancellationToken)
   - Use `BTreeMap` instead of `HashMap` (no_std)
   - Keep UUID/rich context as optional extensions in server crate

3. **Update macros to target `turbomcp_core::McpHandler`**
   - Generate `impl turbomcp_core::McpHandler` instead of server-specific
   - Keep extension trait generation for runtime methods

4. **Unify WASM implementation**
   - `turbomcp-wasm` implements `McpHandlerWasm` extension
   - Same user code works on both platforms

## Success Criteria

- [ ] `cargo check --workspace` passes
- [ ] `cargo check --workspace --target wasm32-unknown-unknown` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace` passes with `-D warnings`
- [ ] Same `#[server]` code compiles for native AND WASM
- [ ] Example server runs on STDIO
- [ ] Example server runs on HTTP
- [ ] Example server runs on Cloudflare Workers
- [ ] No `unsafe` code in core crates
- [ ] Documentation complete

## References

- [Official MCP Rust SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk)
- [MCP Specification 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18)
- [Rust Async Traits (Rust 1.75+)](https://doc.rust-lang.org/book/ch17-05-traits-for-async.html)
- [embedded-hal Async Design](https://docs.rs/embedded-hal-async/latest/embedded_hal_async/)
- [Tower Service Abstraction](https://leapcell.io/blog/unpacking-the-tower-abstraction-layer-in-axum-and-tonic)
- [wasm-bindgen Send/Sync Discussion](https://github.com/rustwasm/wasm-bindgen/issues/2753)
- [Cross-Platform WASM/Native Patterns](https://users.rust-lang.org/t/code-patterns-for-working-with-async-in-cross-platform-wasm-native-context/130558)
- [Rust Security Best Practices 2025](https://corgea.com/Learn/rust-security-best-practices-2025)
- [JSON-RPC Security Best Practices](https://json-rpc.dev/learn/best-practices)

---

*Document created: 2026-01-15*
*Based on deep research of Rust async patterns, MCP SDKs, and cross-platform design*
