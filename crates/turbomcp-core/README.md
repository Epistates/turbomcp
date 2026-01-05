# turbomcp-core

Core MCP types and primitives - `no_std` compatible for WASM targets.

## Overview

This crate provides the foundational types for the Model Context Protocol (MCP) that can be used in `no_std` environments including WebAssembly. It is part of the TurboMCP v3.0 architecture.

## Features

- **`std`** (default): Enable standard library support
- **`rich-errors`**: Enable UUID-based error tracking (requires `std`)
- **`wasm`**: Enable WASM-specific optimizations

## no_std Usage

```toml
[dependencies]
turbomcp-core = { version = "3.0", default-features = false }
```

## What's Included

- **Types**: Core MCP types (Tool, Resource, Prompt, Content, Capabilities)
- **Error**: Unified `McpError` type with JSON-RPC code mapping
- **JSON-RPC**: JSON-RPC 2.0 request/response types

## Example

```rust
use turbomcp_core::types::{Tool, ToolInputSchema};
use turbomcp_core::error::{McpError, ErrorKind, McpResult};

// Create a tool definition
let tool = Tool::new("calculator")
    .with_description("Performs calculations")
    .with_input_schema(ToolInputSchema::object());

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
