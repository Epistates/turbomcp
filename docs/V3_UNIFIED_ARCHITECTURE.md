# TurboMCP v3 Architecture - Dual-Runtime, Unified-Core

> **Status**: Implementation Reality (Updated 2026-01-20)
> **Previous**: Design Proposal from 2026-01-15
> **Goal**: Pristine, SOTA architecture for native + WASM

## Executive Summary

TurboMCP v3 achieves cross-platform MCP support through a **Dual-Runtime, Unified-Core** architecture:

1. **Unified Core Foundation** - `turbomcp-core` and `turbomcp-types` are `no_std` compatible
2. **Platform-Adaptive Bounds** - Conditional `Send` bounds via `MaybeSend`/`MaybeSync` marker traits
3. **Dual Runtime Implementations** - Native (`turbomcp-server`) and WASM (`turbomcp-wasm`)
4. **Separate But Consistent Macros** - Native `#[server]` and WASM `#[turbomcp_wasm::server]`

## Architectural Decision

### Original Proposal vs. Reality

The original design proposed a single `#[server]` macro that would work for both native and WASM. During implementation, we discovered this is impractical because:

1. **Runtime Divergence**: Native uses `tokio`, WASM uses `wasm-bindgen-futures`
2. **Dependency Isolation**: Users shouldn't pull in `tokio` for WASM builds or `wasm-bindgen` for native
3. **Transport Differences**: Native has STDIO/TCP/Unix sockets; WASM has HTTP/Worker/Fetch API
4. **Authentication**: WASM requires Web Crypto API; Native uses standard Rust crypto

### The Pragmatic Solution

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        Application Layer                                  │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                   │
│   │  Native MCP  │  │  Cloudflare  │  │   Browser    │                   │
│   │   Server     │  │   Worker     │  │   Client     │                   │
│   └──────────────┘  └──────────────┘  └──────────────┘                   │
└──────────────────────────────────────────────────────────────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│    turbomcp     │  │  turbomcp-wasm  │  │ turbomcp-wasm   │
│   (std, tokio)  │  │   (server)      │  │   (browser)     │
│                 │  │                 │  │                 │
│ • #[server]     │  │ • McpServer     │  │ • BrowserClient │
│ • run_stdio()   │  │ • WithAuth      │  │ • FetchTransport│
│ • run_http()    │  │ • Worker integ  │  │                 │
│ • run_tcp()     │  │                 │  │                 │
└─────────────────┘  └─────────────────┘  └─────────────────┘
         │                    │                    │
         └────────────────────┼────────────────────┘
                              ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                     Core Layer (turbomcp-core)                            │
│   • McpHandler trait (unified, platform-adaptive Send bounds)             │
│   • RequestContext (minimal, no_std compatible)                           │
│   • MaybeSend/MaybeSync marker traits                                     │
│   • Authentication traits (Authenticator, CredentialExtractor)            │
│   no_std compatible with alloc                                            │
└──────────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                     Types Layer (turbomcp-types)                          │
│   • ALL MCP type definitions (Tool, Resource, Prompt, ServerInfo)         │
│   • Result types (ToolResult, ResourceResult, PromptResult)               │
│   • Error types (McpError with JSON-RPC codes)                            │
│   Single source of truth - no_std compatible                              │
└──────────────────────────────────────────────────────────────────────────┘
```

## Usage Patterns

### Native Server (tokio-based)

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    #[tool]
    async fn greet(&self, name: String) -> String {
        format!("Hello, {}!", name)
    }
}

#[tokio::main]
async fn main() {
    MyServer.run_stdio().await.unwrap();
}
```

### WASM Server (Cloudflare Workers)

```rust
use turbomcp_wasm::wasm_server::*;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-server", "1.0.0")
        .tool("greet", "Say hello", |args: GreetArgs| async move {
            format!("Hello, {}!", args.name)
        })
        .build();

    server.handle(req).await
}
```

### WASM Server with Authentication

```rust
use turbomcp_wasm::wasm_server::*;
use turbomcp_wasm::auth::CloudflareAccessAuthenticator;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-server", "1.0.0")
        .tool("greet", "Say hello", greet_handler)
        .build();

    // Wrap with Cloudflare Access authentication
    let auth = CloudflareAccessAuthenticator::new("my-team", "my-aud");
    let protected = server.with_auth(auth);

    protected.handle(req).await
}
```

## Key Components

### Unified Handler Trait (`turbomcp-core`)

The `McpHandler` trait uses conditional `Send` bounds via marker traits:

```rust
/// Marker trait that's `Send` on native, nothing on WASM.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}

/// The unified MCP handler trait.
pub trait McpHandler: Clone + MaybeSend + MaybeSync + 'static {
    fn server_info(&self) -> ServerInfo;
    fn list_tools(&self) -> Vec<Tool>;
    // ... other methods with MaybeSend bounds on futures
}
```

### Request Context (`turbomcp-core`)

Minimal, `no_std` compatible context:

```rust
pub struct RequestContext {
    pub request_id: String,
    pub transport: TransportType,
    pub metadata: BTreeMap<String, String>,
    pub principal: Option<Principal>,  // Set after authentication
}
```

### Authentication (`turbomcp-core` + `turbomcp-wasm`)

- **Core traits**: `Authenticator`, `CredentialExtractor`, `Principal` in `turbomcp-core`
- **WASM implementation**: JWT validation via Web Crypto API in `turbomcp-wasm/src/auth/`
- **Native implementation**: Standard Rust crypto in `turbomcp-auth`

## What's Unified vs. What's Separate

### Unified (Shared Foundation)

| Component | Crate | Description |
|-----------|-------|-------------|
| `McpHandler` trait | `turbomcp-core` | Platform-adaptive handler interface |
| `RequestContext` | `turbomcp-core` | Minimal request context |
| MCP Types | `turbomcp-types` | Tool, Resource, Prompt, ServerInfo |
| Auth Traits | `turbomcp-core` | `Authenticator`, `CredentialExtractor`, `Principal` |

### Separate (Platform-Specific)

| Component | Native | WASM |
|-----------|--------|------|
| Crate | `turbomcp` | `turbomcp-wasm` |
| Macro | `#[server]` | `McpServer::builder()` |
| Runtime | `tokio` | `wasm-bindgen-futures` |
| Transports | STDIO, HTTP, TCP, Unix, WebSocket | HTTP (Fetch/Workers) |
| JWT Validation | `jsonwebtoken` crate | Web Crypto API |

## Benefits of This Architecture

1. **Clean Dependencies**: Native users don't pull WASM deps, WASM users don't pull tokio
2. **Platform-Optimal**: Each runtime uses its native async primitives
3. **Shared Logic**: Core types and traits are truly shared
4. **Testability**: Core crates can be tested independently
5. **Maintenance**: Changes to MCP types propagate to both platforms

## Migration from v2

1. **Native servers**: No changes needed (same `#[server]` macro)
2. **WASM servers**: Use `turbomcp-wasm` crate instead of trying to use `turbomcp`
3. **Shared code**: Put handler logic in a separate crate that depends only on `turbomcp-core`

## References

- [MCP Specification 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25)
- [wasm-bindgen Send/Sync Discussion](https://github.com/rustwasm/wasm-bindgen/issues/2753)
- [Cloudflare Workers Rust Guide](https://developers.cloudflare.com/workers/languages/rust/)

---

*Document updated: 2026-01-20*
*Reflects actual v3.0.0-beta.1 implementation*
