# turbomcp-core

Core MCP types and primitives - `no_std` compatible for WASM targets.

## Overview

This crate provides the foundational types for the Model Context Protocol (MCP) that can be used in `no_std` environments including WebAssembly. It is part of the TurboMCP v3.0 architecture.

## Features

- **`std`** (default): Enable standard library support
- **`rich-errors`**: Enable UUID-based error tracking (requires `std`)
- **`wasm`**: Enable WASM-specific optimizations
- **`zero-copy`**: Enable rkyv zero-copy serialization for internal message passing

## no_std Usage

```toml
[dependencies]
turbomcp-core = { version = "3.1", default-features = false }
```

## What's Included

- **Types**: Core MCP types re-exported from `turbomcp-types` (Tool, Resource, Prompt, Content, ServerInfo, etc.)
- **Error**: Unified `McpError` type with JSON-RPC code mapping
- **JSON-RPC**: JSON-RPC 2.0 request/response types
- **Handler**: Unified `McpHandler` trait with platform-adaptive `MaybeSend`/`MaybeSync` bounds
- **Auth**: Portable authentication primitives (`Authenticator`, `Credential`, `Principal`)

## Example

```rust
use turbomcp_core::{Tool, ToolInputSchema};
use turbomcp_core::{McpError, McpResult};

// Create a tool definition
let tool = Tool::new("calculator", "Performs calculations")
    .with_schema(ToolInputSchema::default());

// Handle errors
fn my_handler() -> McpResult<String> {
    Err(McpError::tool_not_found("unknown_tool"))
}
```

## Architecture

This crate is the foundation of the TurboMCP v3 architecture:

```
turbomcp-core (no_std)
    └── turbomcp-protocol (async runtime)
        └── turbomcp-server
        └── turbomcp-client
```

## License

MIT
