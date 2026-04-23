# turbomcp-server Migration Guide

This guide covers breaking changes specific to the `turbomcp-server` crate. For workspace-wide migration context, see the [top-level MIGRATION.md](../../MIGRATION.md).

---

## v2.x to v3.0

### Unified error types

In v2.x, the server crate exposed its own error and result types. In v3.x, error handling is unified under a single canonical type defined in `turbomcp-core` and re-exported by every crate.

```rust
// Before (v2.x)
use turbomcp_server::{ServerError, ServerResult};

fn my_handler() -> ServerResult<Value> {
    Err(ServerError::Internal("failed".to_string()))
}

// After (v3.x)
use turbomcp_server::prelude::*; // re-exports McpError and McpResult

fn my_handler() -> McpResult<Value> {
    Err(McpError::internal("failed"))
}
```

`McpError` and `McpResult<T>` are also available via `turbomcp::prelude::*` or directly from `turbomcp_core::error`.

### ProtocolConfig replaces ProtocolVersionConfig

`ProtocolVersionConfig` does not exist in v3.x. Protocol negotiation is configured through `ProtocolConfig` on `ServerConfig`.

```rust
use turbomcp_server::{ProtocolConfig, ServerConfig};
use turbomcp_server::config::ProtocolVersion;

// Default (v3.1+): accepts every stable spec version, prefers the latest.
// Fallback is disabled; unknown versions are rejected.
let config = ServerConfig::new();

// Strict mode: only accept a single version, reject others
let config = ServerConfig::builder()
    .protocol(ProtocolConfig::strict(ProtocolVersion::LATEST.clone()))
    .build();

// Explicit multi-version (equivalent to the default)
let config = ServerConfig::builder()
    .protocol(ProtocolConfig::multi_version())
    .build();

// Hand-built: preferred version with fallback for unknown clients
let config = ServerConfig::builder()
    .protocol(ProtocolConfig {
        preferred_version: ProtocolVersion::LATEST.clone(),
        supported_versions: ProtocolVersion::STABLE.to_vec(),
        allow_fallback: true,
    })
    .build();
```

`ProtocolConfig` fields:

| Field | Type | Default | Description |
|---|---|---|---|
| `preferred_version` | `ProtocolVersion` | `ProtocolVersion::LATEST` (`2025-11-25`) | Version offered when client omits one or (with fallback) sends an unknown one |
| `supported_versions` | `Vec<ProtocolVersion>` | `ProtocolVersion::STABLE.to_vec()` (all stable versions) | Versions this server accepts |
| `allow_fallback` | `bool` | `false` | Offer preferred version when client's is unsupported |

### New channel transport feature

v3.x adds an in-process channel transport for zero-overhead communication between components in the same process. Enable with the `channel` feature flag:

```toml
turbomcp-server = { version = "3.1.1", features = ["channel"] }
```

### ServerConfig now has try_build for validation

`ServerConfigBuilder` gains `try_build()` alongside `build()`. Use `try_build()` when you want configuration errors surfaced at startup rather than at runtime:

```rust
use turbomcp_server::{ServerConfig, ConfigValidationError};

let config = ServerConfig::builder()
    .max_message_size(1024 * 1024)
    .try_build()
    .expect("invalid server configuration");
```

`try_build()` rejects: `max_message_size` below 1024 bytes, `max_requests` of 0, zero-duration rate limit window, and connection limits where all four transport limits are 0.

### Feature flag defaults unchanged

The default feature is `stdio`. This has not changed between v2.x and v3.x. If you were explicitly opting into transport features, your `Cargo.toml` entries continue to work:

```toml
# These all work the same as before
turbomcp-server = { version = "3.1.1", features = ["http"] }
turbomcp-server = { version = "3.1.1", features = ["stdio", "http", "websocket", "tcp"] }
turbomcp-server = { version = "3.1.1", features = ["full"] }
```

---

## v1.x to v2.0

### Import paths: turbomcp-core merged into turbomcp-protocol

The `turbomcp-core` crate was absorbed into `turbomcp-protocol` in v2.0. Any imports that referenced `turbomcp_core` directly need to move to `turbomcp_protocol` or use the re-exports from `turbomcp_server`.

```rust
// Before (v1.x) - if depending on turbomcp-core directly
use turbomcp_core::types::Tool;

// After (v3.x) - canonical home is turbomcp-types
use turbomcp_types::Tool;
// or, preferred:
use turbomcp_server::prelude::*;
```

Note: in v3.x, `turbomcp-core` was reintroduced as a `no_std` foundation layer, so `turbomcp_core` import paths are valid again. The `turbomcp_server::prelude` re-exports from whichever internal crate is canonical, so using `prelude::*` remains the most stable choice across versions.

### Authentication extracted to turbomcp-auth

OAuth and JWT support moved from `turbomcp-server` into the standalone `turbomcp-auth` crate. If you were importing auth types from `turbomcp_server::auth`, add the `turbomcp-auth` dependency and update the import paths.

```toml
# Before (v1.x) - auth was bundled in turbomcp-server
[dependencies]
turbomcp-server = "1.x"

# After (v2.x) - auth is a separate optional crate
[dependencies]
turbomcp-server = "2.0"
turbomcp-auth = "2.0"  # add if you use OAuth/JWT features
```

### with_middleware is on MiddlewareStack, not ServerBuilder

`ServerBuilder` does not have a `with_middleware` method. Middleware is composed through `MiddlewareStack`, which wraps a handler and implements `McpHandler` itself:

```rust
use turbomcp_server::{MiddlewareStack, McpServerExt, Transport};

// Wrap the handler, then layer middleware onto it
let stack = MiddlewareStack::new(MyServer).with_middleware(my_middleware);

stack.builder()
    .transport(Transport::http("0.0.0.0:8080"))
    .serve()
    .await?;
```

### ServerBuilder API reference (v3.1)

The following methods are available on `ServerBuilder<H>`:

| Method | Description |
|---|---|
| `.transport(Transport)` | Set the transport (default: `Transport::Stdio`) |
| `.with_rate_limit(u32, Duration)` | Enable token-bucket rate limiting (per client) |
| `.with_connection_limit(usize)` | Cap concurrent connections across TCP/HTTP/WS/Unix |
| `.with_graceful_shutdown(Duration)` | Wait up to this duration for in-flight requests on shutdown (HTTP transport) |
| `.with_max_message_size(usize)` | Reject messages larger than this (default: 10 MB) |
| `.with_protocol(ProtocolConfig)` | Configure protocol version negotiation |
| `.with_allowed_origin(impl Into<String>)` | Allow a specific HTTP origin |
| `.with_origin_validation(OriginValidationConfig)` | Replace the full origin config |
| `.allow_localhost_origins(bool)` | Accept/deny localhost origins |
| `.allow_any_origin(bool)` | Disable origin checks entirely |
| `.with_config(ServerConfig)` | Apply a fully constructed `ServerConfig` |
| `.serve()` | Start the server (async, blocks until shutdown) |
| `.into_axum_router()` | Return an `axum::Router` for BYO server integration (requires `http` feature) |
| `.into_service()` | Return a Tower service (requires `http` feature) |
| `.handler()` / `.into_handler()` | Borrow / consume the underlying handler |

Available transports via `Transport`:

| Constructor | Feature flag | Notes |
|---|---|---|
| `Transport::stdio()` | `stdio` | Default; used by Claude Desktop |
| `Transport::http(addr)` | `http` | JSON-RPC over HTTP POST |
| `Transport::websocket(addr)` | `websocket` | Bidirectional; implies `http` |
| `Transport::tcp(addr)` | `tcp` | Line-framed JSON-RPC over TCP |
| `Transport::unix(path)` | `unix` | Unix domain sockets |
