# TurboMCP Protocol

[![Crates.io](https://img.shields.io/crates/v/turbomcp-protocol.svg)](https://crates.io/crates/turbomcp-protocol)
[![Documentation](https://docs.rs/turbomcp-protocol/badge.svg)](https://docs.rs/turbomcp-protocol)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Model Context Protocol (MCP) specification implementation with JSON-RPC 2.0 and runtime schema validation.

## Table of Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [Architecture](#architecture)
- [MCP Message Types](#mcp-message-types)
- [Usage](#usage)
- [Message Flow](#message-flow)
- [Feature Flags](#feature-flags)
- [Supported MCP Methods](#supported-mcp-methods)
- [Integration](#integration)

## Overview

`turbomcp-protocol` provides a specification-compliant implementation of the Model Context Protocol (MCP). This crate handles protocol-level concerns including message formatting, capability negotiation, and runtime validation.

## Key Features

### MCP Specification Support
- MCP specification implementation with current message types
- Tools, resources, prompts, and capabilities support
- Capability negotiation with feature detection and handshake
- Version compatibility support

### JSON-RPC 2.0 Implementation
- Compliant message format with request, response, and notification handling
- ID correlation for automatic request/response matching
- Standard JSON-RPC error codes and extensions
- Support for batch request/response operations

### Runtime Schema Validation
- JSON Schema validation using `jsonschema` crate
- Rust type definitions for MCP message types
- Tool and resource parameter validation
- Schema generation from Rust types

### Capability Management
- Type-State Capability Builders - Compile-time validated capability configuration
- Server capabilities for tools, resources, prompts declarations
- Client capabilities including sampling, roots, progress reporting
- Feature negotiation with capability matching
- Support for custom capability extensions

### MCP Enhanced Features
- Bidirectional communication for server-initiated requests to clients
- Elicitation support for server-requested structured input from users
- Completion context with references and metadata
- Resource templates for dynamic resource generation with parameters
- Ping/keepalive for connection health monitoring

## Architecture

```
┌─────────────────────────────────────────────┐
│            TurboMCP Protocol                │
├─────────────────────────────────────────────┤
│ MCP Message Types                          │
│ ├── InitializeRequest/InitializeResult     │
│ ├── Tool/Resource/Prompt messages          │
│ ├── Capability negotiation               │
│ └── Notification handling                 │
├─────────────────────────────────────────────┤
│ JSON-RPC 2.0 Layer                        │
│ ├── Request/Response correlation          │
│ ├── ID generation and tracking           │
│ ├── Error code standardization           │
│ └── Batch message processing             │
├─────────────────────────────────────────────┤
│ Schema Validation                          │
│ ├── Runtime JSON schema validation       │
│ ├── Parameter type checking              │
│ ├── Response format validation           │
│ └── Custom schema extension support      │
└─────────────────────────────────────────────┘
```

## MCP Message Types

### Core Message Types

```rust
use turbomcp_protocol::{
    // Re-exported for convenience (most common types)
    InitializeRequest, InitializeResult,
    CallToolRequest, CallToolResult,
    ReadResourceRequest, ReadResourceResult,
    GetPromptRequest, GetPromptResult,
    // List types available via types module
};

use turbomcp_protocol::types::{
    ListToolsRequest, ListToolsResult,
    ListResourcesRequest, ListResourcesResult,
    ListPromptsRequest, ListPromptsResult,
    Tool, Resource, Prompt,
};
```

### MCP Enhanced Types

```rust
use turbomcp_protocol::{
    // Bidirectional communication support (trait)
    ServerToClientRequests,
};

use turbomcp_protocol::types::{
    // Elicitation - Server requests user input
    ElicitRequest, ElicitResult,

    // Completion - Intelligent autocompletion
    CompleteRequest, CompletionResponse,

    // Resource Templates - Dynamic resources
    ListResourceTemplatesRequest, ListResourceTemplatesResult,

    // Ping - Bidirectional health monitoring
    PingRequest, PingResult,
};
```

### JSON-RPC Infrastructure

```rust
use turbomcp_protocol::{
    JsonRpcRequest, JsonRpcResponse, JsonRpcNotification,
    JsonRpcError, JsonRpcErrorCode, RequestId,
};
```

## Usage

### Basic Protocol Handling

```rust
use turbomcp_protocol::{
    JsonRpcRequest, JsonRpcResponse, InitializeRequest,
    ListToolsRequest, Error as McpError
};

// Parse incoming JSON-RPC request
let json_data = r#"{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "test-client", "version": "2.0.0"}
    }
}"#;

let request: JsonRpcRequest = serde_json::from_str(json_data)?;

// Handle specific message types
match request.method.as_str() {
    "initialize" => {
        let params = request.params.unwrap_or_default();
        let init_req: InitializeRequest = serde_json::from_value(params)?;
        // Process initialization
    },
    "tools/list" => {
        let params = request.params.unwrap_or_default();
        let tools_req: ListToolsRequest = serde_json::from_value(params)?;
        // Process tools list request
    },
    _ => {
        // Handle unknown method
    }
}
```

### Message Validation

```rust
use turbomcp_protocol::{
    JsonRpcRequest,
    validation::{ProtocolValidator, ValidationResult}
};

// Create a validator with default rules
let validator = ProtocolValidator::default();

// Parse and validate a JSON-RPC request
let json_data = r#"{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {"name": "add", "arguments": {"a": 5, "b": 3}}
}"#;

let request: JsonRpcRequest = serde_json::from_str(json_data)?;
let result = validator.validate_request(&request);

match result {
    ValidationResult::Valid => {
        println!("Request is valid");
    },
    ValidationResult::ValidWithWarnings(warnings) => {
        println!("Request valid with {} warnings", warnings.len());
    },
    ValidationResult::Invalid(errors) => {
        eprintln!("Request invalid: {} errors", errors.len());
    }
}
```

### Type-State Capability Builders

```rust
use turbomcp_protocol::capabilities::builders::{
    ServerCapabilitiesBuilder, ClientCapabilitiesBuilder
};

// Compile-time validated server capabilities
let server_caps = ServerCapabilitiesBuilder::new()
    .enable_tools()                    // Enable tools capability
    .enable_resources()                // Enable resources capability
    .enable_prompts()                  // Enable prompts capability
    .enable_tool_list_changed()        // ✅ Only available when tools enabled
    .enable_resources_subscribe()      // ✅ Only available when resources enabled
    .enable_resources_list_changed()   // ✅ Only available when resources enabled
    .build();

// Opt-out client capabilities (all enabled by default)
let client_caps = ClientCapabilitiesBuilder::new()
    .enable_roots_list_changed()       // Configure sub-capabilities
    .build();                          // All capabilities enabled!

// Opt-in pattern for restrictive clients
let minimal_client = ClientCapabilitiesBuilder::minimal()
    .enable_sampling()                 // Only enable what you need
    .enable_roots()
    .build();
```

### Traditional Capability Negotiation

```rust
use turbomcp_protocol::{
    ServerCapabilities, ClientCapabilities,
    types::{ToolsCapabilities, ResourcesCapabilities, PromptsCapabilities, RootsCapabilities}
};

// Traditional approach (still supported)
let server_caps = ServerCapabilities {
    tools: Some(ToolsCapabilities {
        list_changed: Some(true),
    }),
    resources: Some(ResourcesCapabilities {
        subscribe: Some(true),
        list_changed: Some(true),
    }),
    prompts: Some(PromptsCapabilities {
        list_changed: Some(false),
    }),
    experimental: None,
    ..Default::default()
};

// Define client capabilities
let client_caps = ClientCapabilities {
    sampling: None,
    roots: Some(RootsCapabilities {
        list_changed: Some(true),
    }),
    experimental: None,
    ..Default::default()
};
```

### Error Handling

The protocol crate provides `Error`, a rich MCP 2025-06-18 specification-compliant error type with comprehensive context and observability support.

#### Understanding `Box<Error>`

**All error constructors return `Box<Error>`** for important architectural reasons:

```rust
use turbomcp_protocol::Error;

// Constructors return Box<Error>, not Error
let err: Box<Error> = Error::tool_not_found("calculator");
let err: Box<Error> = Error::invalid_params("Email required");
```

**Why `Box<Error>`?**

1. **Cheap Cloning**: Errors clone efficiently across async boundaries
2. **Rich Context Preservation**: Full error chain, metadata, and backtrace
3. **Observability Integration**: Seamless tracing and metrics
4. **Memory Efficiency**: Error type is large (contains UUID, context, backtrace) - boxing keeps it off the stack

#### Creating Errors

```rust
use turbomcp_protocol::{Error, ErrorKind};

// MCP specification errors (map to standard error codes)
let err = Error::tool_not_found("calculator");              // -32001
let err = Error::tool_execution_failed("calc", "div by 0"); // -32002
let err = Error::prompt_not_found("code_review");           // -32003
let err = Error::resource_not_found("file:///missing");     // -32004
let err = Error::resource_access_denied("file:///etc/passwd", "forbidden"); // -32005
let err = Error::invalid_params("Email must be valid");     // -32602
let err = Error::user_rejected("User declined sampling");   // -1

// Add rich context with builder pattern
let err = Error::internal("Database error")
    .with_operation("user_create")
    .with_component("postgres_repository")
    .with_request_id("req-123")
    .with_context("user_id", user_id)
    .with_context("table", "users");

// Error chaining for root cause analysis
let database_error = Error::internal("Connection pool exhausted");
let app_error = Error::unavailable("Service temporarily unavailable")
    .with_source(database_error);
```

#### Working with JSON-RPC Errors

```rust
use turbomcp_protocol::{JsonRpcError, JsonRpcErrorCode, Error};

// Create JSON-RPC errors directly
fn handle_tool_error(error: &str) -> JsonRpcError {
    JsonRpcError {
        code: JsonRpcErrorCode::InvalidParams,
        message: format!("Tool validation failed: {}", error),
        data: None,
    }
}

// Convert protocol Error to JSON-RPC error code
let err = Error::tool_not_found("calculator");
let code = err.jsonrpc_error_code();  // -32001
let http = err.http_status_code();    // 404

// Create Error from JSON-RPC error code (preserves semantics)
let err = Error::rpc(-32001, "Tool 'calculator' not found");
assert_eq!(err.kind, ErrorKind::ToolNotFound);
```

#### Error Properties

```rust
use turbomcp_protocol::Error;

let err = Error::timeout("Request took too long");

// Check error characteristics
if err.is_retryable() {
    // Retry the operation
}

if err.is_temporary() {
    // Wait and retry
}

// Get HTTP status code for REST APIs
let status = err.http_status_code();  // 408

// Get MCP-compliant JSON-RPC error code
let code = err.jsonrpc_error_code();  // -32012
```

#### Integration with Application Layer

If you're using the main `turbomcp` crate, you typically use `McpError` in your tool handlers. The server layer automatically converts to `Box<Error>`:

```rust
// In your tool handler (turbomcp crate)
use turbomcp::{McpError, McpResult};

#[tool("My tool")]
async fn my_tool(&self) -> McpResult<String> {
    Err(McpError::Tool("Something failed".into()))  // Simple error
}

// Server layer converts to:
// ServerError::Protocol(Error::tool_execution_failed("my_tool", "Something failed"))
```

See the [turbomcp crate error handling docs](../turbomcp/README.md#error-handling) for the complete error architecture.

### Custom Message Types

```rust
use turbomcp_protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use serde::{Serialize, Deserialize};

// Define custom message types
#[derive(Serialize, Deserialize)]
struct CustomRequest {
    custom_field: String,
    optional_data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct CustomResponse {
    result: String,
    metadata: serde_json::Value,
}

// Create custom JSON-RPC messages
fn create_custom_request(id: RequestId, params: CustomRequest) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id,
        method: "custom/method".to_string(),
        params: serde_json::to_value(params).unwrap(),
    }
}
```

## Message Flow

```mermaid
sequenceDiagram
    participant Client
    participant Protocol as turbomcp-protocol
    participant Server
    
    Client->>Protocol: Raw JSON message
    Protocol->>Protocol: Parse JSON-RPC
    Protocol->>Protocol: Validate message format
    Protocol->>Protocol: Extract MCP message
    Protocol->>Protocol: Validate against schema
    Protocol->>Server: Typed MCP message
    Server->>Protocol: Typed MCP response
    Protocol->>Protocol: Serialize response
    Protocol->>Protocol: Wrap in JSON-RPC
    Protocol->>Client: JSON response
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `simd` | SIMD-accelerated JSON parsing (simd-json, sonic-rs) | ✅ |
| `std` | Standard library support (always enabled with no-std support via wasm feature) | ✅ |
| `zero-copy` | Zero-copy message handling with serde serialization | ❌ |
| `messagepack` | MessagePack serialization support | ❌ |
| `tracing` | OpenTelemetry tracing integration | ❌ |
| `metrics` | Prometheus metrics collection | ❌ |
| `mmap` | Memory-mapped file support | ❌ |
| `lock-free` | Lock-free data structures (experimental, requires unsafe) | ❌ |
| `fancy-errors` | Rich error reporting with miette | ❌ |
| `wasm` | WebAssembly support (no_std compatible) | ❌ |

## Supported MCP Methods

### Core Methods

- `initialize` - Protocol initialization and capability negotiation
- `initialized` - Initialization completion notification

### Tool Methods

- `tools/list` - List available tools
- `tools/call` - Execute a tool with parameters

### Resource Methods

- `resources/list` - List available resources
- `resources/read` - Read resource content
- `resources/updated` - Resource change notification

### Prompt Methods

- `prompts/list` - List available prompts
- `prompts/get` - Get prompt content

### Capability Methods

- `capabilities/changed` - Capability change notification

## Integration

### With TurboMCP Framework

Protocol handling is automatic when using the main framework:

```rust
use turbomcp::prelude::*;

#[server]
impl MyServer {
    #[tool("Add numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        // Protocol parsing and validation handled automatically
        Ok(a + b)
    }
}
```

### Direct Protocol Usage

For custom implementations or integrations:

```rust
use turbomcp_protocol::{JsonRpcRequest, JsonRpcResponse};

struct CustomProtocolHandler;

impl CustomProtocolHandler {
    async fn handle_message(&self, raw_json: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Parse JSON-RPC message
        let request: JsonRpcRequest = serde_json::from_str(raw_json)?;
        
        // Handle based on method
        let response = match request.method.as_str() {
            "tools/list" => self.handle_tools_list(request).await?,
            "tools/call" => self.handle_tools_call(request).await?,
            _ => return Err("Unknown method".into()),
        };
        
        // Serialize response
        Ok(serde_json::to_string(&response)?)
    }
}
```

## Development

### Building

```bash
# Build with all features
cargo build --features validation,extensions,batch

# Build minimal (no validation)
cargo build --no-default-features
```

### Testing

```bash
# Run protocol compliance tests
cargo test

# Test with all message types
cargo test --features extensions

# Validate against MCP specification
cargo test test_mcp_compliance
```

### Schema Validation

```bash
# Generate JSON schemas from Rust types
cargo run --example generate_schemas

# Validate example messages
cargo test test_message_validation
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework (uses this crate)
- **[turbomcp-transport](../turbomcp-transport/)** - Transport layer
- **[turbomcp-server](../turbomcp-server/)** - Server framework

**Note:** In v2.0.0, `turbomcp-core` was merged into this crate to eliminate circular dependencies and improve cohesion.

## External Resources

- **[MCP Specification](https://modelcontextprotocol.io/)** - Official protocol specification
- **[JSON-RPC 2.0](https://www.jsonrpc.org/specification)** - JSON-RPC specification
- **[JSON Schema](https://json-schema.org/)** - Schema validation specification

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) Rust SDK for the Model Context Protocol.*