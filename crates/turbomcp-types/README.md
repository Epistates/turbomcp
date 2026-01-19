# turbomcp-types

Core types for the TurboMCP SDK - the foundation of MCP server development.

## Overview

This crate provides all shared types used across the TurboMCP ecosystem:

- **Content types**: `Content`, `TextContent`, `ImageContent`, etc.
- **Result types**: `ToolResult`, `ResourceResult`, `PromptResult`
- **Definition types**: `Tool`, `Resource`, `Prompt`, `ServerInfo`
- **Conversion traits**: `IntoToolResult`, `IntoResourceResult`, `IntoPromptResult`

For error handling, use `turbomcp_core::error::{McpError, McpResult}`.

## Features

- **`std`** (default): Enable standard library support
- **`alloc`**: Allocator support without full std (for no_std + alloc environments)
- **`schema`**: JSON Schema generation for tool input schemas

## Design Principles

1. **Single Source of Truth**: These types are the canonical definitions
2. **Ergonomic by Default**: Common operations are one-liners
3. **MCP 2025-11-25 Compliant**: Full spec support
4. **no_std Compatible**: Works in WASM and embedded environments

## Quick Start

```rust
use turbomcp_types::*;

// Create a tool result
let result = ToolResult::text("Hello, world!");

// Create an error result
let error = ToolResult::error("Something went wrong");

// Create a JSON result with structured content
let json_result = ToolResult::json(&serde_json::json!({"key": "value"})).unwrap();

// Create a resource result
let resource = ResourceResult::text("file:///example.txt", "File contents here");

// Create a prompt result
let prompt = PromptResult::user("Hello!")
    .add_assistant("How can I help?")
    .with_description("A greeting prompt");
```

## no_std Usage

```toml
[dependencies]
turbomcp-types = { version = "3.0", default-features = false, features = ["alloc"] }
```

## License

MIT
