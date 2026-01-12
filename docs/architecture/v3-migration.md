# v3 Migration Guide

Complete guide for migrating from TurboMCP v2.x to v3.0.

## Overview

TurboMCP 3.0 represents a major modular architecture redesign with:

- **Unified error types** - `McpError` replaces `ServerError`, `ClientError`
- **Modular transports** - Individual crates for each transport
- **`no_std` core** - Foundation layer works in WASM and embedded
- **Tower middleware** - Native Tower integration replaces plugin system
- **MCP 2025-11-25** - Full compliance with latest specification

## Quick Migration

### Using Compatibility Crate

For gradual migration, use the compatibility crate:

```toml
[dependencies]
turbomcp = "3.0"
turbomcp-compat = "3.0"  # Provides deprecated v2 aliases
```

```rust
// Deprecation warnings will guide migration
use turbomcp_compat::v2::{ServerError, ServerResult};
```

### Direct Migration

```toml
# Before (v2.x)
[dependencies]
turbomcp = "2.x"

# After (v3.x)
[dependencies]
turbomcp = "3.0"
```

## Breaking Changes

### 1. Error Type Unification

**Before (v2.x):**

```rust
use turbomcp_server::{ServerError, ServerResult};
use turbomcp_client::{ClientError, ClientResult};

fn server_handler() -> ServerResult<Value> {
    Err(ServerError::internal("failed"))
}

fn client_call() -> ClientResult<Value> {
    Err(ClientError::connection("failed"))
}
```

**After (v3.x):**

```rust
use turbomcp::{McpError, McpResult};

fn server_handler() -> McpResult<Value> {
    Err(McpError::internal("failed"))
}

fn client_call() -> McpResult<Value> {
    Err(McpError::internal("connection failed"))
}
```

**Type Mapping:**

| v2.x Type | v3.x Type |
|-----------|-----------|
| `ServerError` | `McpError` |
| `ServerResult<T>` | `McpResult<T>` |
| `ClientError` | `McpError` |
| `ClientResult<T>` | `McpResult<T>` |
| `Error` (protocol) | `McpError` |
| `Claims` | `AuthContext` |

### 2. Modular Transport Architecture

**Before (v2.x):**

```toml
# Monolithic transport
turbomcp = { version = "2.x", features = ["http", "websocket"] }
```

**After (v3.x):**

```toml
# Same feature flags work (for compatibility)
turbomcp = { version = "3.0", features = ["http", "websocket"] }

# Or use individual crates directly
turbomcp-http = "3.0"
turbomcp-websocket = "3.0"
```

**New Transport Crates:**

| Crate | Feature | Description |
|-------|---------|-------------|
| `turbomcp-stdio` | `stdio` | Standard I/O (default) |
| `turbomcp-http` | `http` | HTTP/SSE |
| `turbomcp-websocket` | `websocket` | WebSocket |
| `turbomcp-tcp` | `tcp` | TCP sockets |
| `turbomcp-unix` | `unix` | Unix sockets |
| `turbomcp-grpc` | `grpc` | gRPC (new) |

### 3. Feature Flag Simplification

**Removed Features (now always enabled):**

| Old Feature | Now Always Available |
|-------------|---------------------|
| `mcp-icons` | `Icons`, `IconTheme` |
| `mcp-url-elicitation` | `URLElicitRequestParams` |
| `mcp-sampling-tools` | `tools`/`tool_choice` |
| `mcp-enum-improvements` | `EnumSchema`, `EnumOption` |
| `mcp-draft` | `description` on `Implementation` |

**Before (v2.x):**

```toml
turbomcp-protocol = { version = "2.x", features = ["mcp-icons", "mcp-url-elicitation"] }
```

**After (v3.x):**

```toml
# No feature flags needed - all MCP 2025-11-25 features are always on
turbomcp-protocol = "3.0"
```

**Renamed Features:**

| Old Name | New Name |
|----------|----------|
| `mcp-tasks` | `experimental-tasks` |

### 4. Tower Middleware Replaces Plugin System

**Before (v2.x):**

```rust
use turbomcp_client::plugins::{Plugin, PluginContext};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn on_request(&self, ctx: &mut PluginContext) {
        // Plugin logic
    }
}

client.register_plugin(MyPlugin);
```

**After (v3.x):**

```rust
use tower::{Layer, Service, ServiceBuilder};

struct MyLayer;

impl<S> Layer<S> for MyLayer {
    type Service = MyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MyService { inner }
    }
}

let service = ServiceBuilder::new()
    .layer(MyLayer)
    .service(client);
```

### 5. Authentication Changes

**Before (v2.x):**

```rust
use turbomcp_server::middleware::Claims;
```

**After (v3.x):**

```rust
use turbomcp_auth::AuthContext;
```

### 6. `no_std` Core Layer

The `turbomcp-core` crate is now `no_std` compatible:

```toml
# For no_std environments (WASM, embedded)
turbomcp-core = { version = "3.0", default-features = false }

# For standard environments (default)
turbomcp-core = "3.0"
```

## Step-by-Step Migration

### Step 1: Update Dependencies

```toml
[dependencies]
turbomcp = "3.0"
tokio = { version = "1", features = ["full"] }
```

### Step 2: Update Imports

```rust
// Before
use turbomcp_server::{ServerError, ServerResult};
use turbomcp_client::{ClientError, ClientResult};

// After
use turbomcp::{McpError, McpResult};
```

### Step 3: Update Error Handling

```rust
// Before
fn handler() -> ServerResult<String> {
    Err(ServerError::internal("error"))
}

// After
fn handler() -> McpResult<String> {
    Err(McpError::internal("error"))
}
```

### Step 4: Update Middleware (if using plugins)

```rust
// Before
client.register_plugin(MyPlugin);

// After
use tower::ServiceBuilder;

let client = ServiceBuilder::new()
    .layer(MyLayer)
    .service(base_client);
```

### Step 5: Remove Deprecated Feature Flags

```toml
# Before
turbomcp = { version = "2.x", features = ["mcp-icons", "mcp-url-elicitation"] }

# After
turbomcp = "3.0"  # These are always enabled now
```

### Step 6: Update Tests

```rust
// Before
#[test]
fn test_error() {
    let err = ServerError::internal("test");
    assert!(matches!(err, ServerError::Internal(_)));
}

// After
#[test]
fn test_error() {
    let err = McpError::internal("test");
    assert_eq!(err.kind(), ErrorKind::InternalError);
}
```

## New Features to Adopt

### Wire Codecs

```rust
use turbomcp_wire::{Codec, SimdJsonCodec};

let codec = SimdJsonCodec::new();  // 2-4x faster JSON
```

### gRPC Transport

```toml
turbomcp = { version = "3.0", features = ["grpc"] }
```

```rust
use turbomcp_grpc::server::McpGrpcServer;

let server = McpGrpcServer::builder()
    .server_info("my-server", "1.0.0")
    .build();
```

### OpenTelemetry

```rust
use turbomcp_telemetry::{TelemetryConfig, TelemetryLayer};

let config = TelemetryConfig::builder()
    .service_name("my-server")
    .otlp_endpoint("http://jaeger:4317")
    .build();
```

### WASM Support

```javascript
import init, { McpClient } from 'turbomcp-wasm';

await init();
const client = new McpClient("https://api.example.com/mcp");
```

## Common Migration Issues

### Issue: `ServerError` not found

```rust
// Error: cannot find value `ServerError` in module `turbomcp_server`

// Solution: Use McpError
use turbomcp::McpError;
Err(McpError::internal("error"))
```

### Issue: Feature flag not found

```toml
# Error: feature `mcp-icons` not found

# Solution: Remove the feature flag (now always enabled)
turbomcp = "3.0"
```

### Issue: Plugin trait not found

```rust
// Error: cannot find trait `Plugin`

// Solution: Use Tower middleware instead
use tower::{Layer, Service};
```

### Issue: Claims type moved

```rust
// Error: cannot find type `Claims`

// Solution: Use AuthContext from turbomcp-auth
use turbomcp_auth::AuthContext;
```

## Deprecation Timeline

| Version | Status |
|---------|--------|
| v3.0.0 | Compat crate provides deprecated aliases |
| v3.1.0 | Deprecation warnings become errors |
| v4.0.0 | Compat crate removed |

## Version Compatibility

| TurboMCP | Rust | MCP Spec | Status |
|----------|------|----------|--------|
| 3.0.x | 1.89.0+ | 2025-11-25 | Current |
| 2.3.x | 1.89.0+ | 2025-06-18 | Maintenance |
| 2.0.x | 1.89.0+ | 2024-11-05 | EOL |

## Resources

- **[MIGRATION.md](https://github.com/turbomcp/turbomcp/blob/main/MIGRATION.md)** - Full migration guide
- **[CHANGELOG.md](https://github.com/turbomcp/turbomcp/blob/main/CHANGELOG.md)** - Version history
- **[Error Handling Guide](../guide/error-handling.md)** - McpError details
- **[Tower Middleware Guide](../guide/tower-middleware.md)** - Middleware patterns

## Getting Help

- **GitHub Issues** - Report migration problems
- **GitHub Discussions** - Ask questions
- **API Documentation** - https://docs.rs/turbomcp
