# Core Types API Reference

The `turbomcp-core` crate provides foundational MCP types that are `no_std` compatible for WASM and embedded environments.

## Overview

This crate is the foundation of the TurboMCP v3 architecture:

```
turbomcp-core (no_std)
    └── turbomcp-protocol (async runtime)
        └── turbomcp-server
        └── turbomcp-client
```

## Installation

```toml
[dependencies]
# With std (default)
turbomcp-core = "3.0"

# For no_std environments
turbomcp-core = { version = "3.0", default-features = false }
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Standard library support | Yes |
| `rich-errors` | UUID-based error tracking | No (requires `std`) |
| `wasm` | WASM-specific optimizations | No |

## Error Types

### McpError

The unified error type for the entire SDK.

```rust
use turbomcp_core::error::{McpError, McpResult, ErrorKind};

// Create errors
let err = McpError::new(ErrorKind::InvalidParams, "Missing field");
let err = McpError::tool_not_found("calculator");
let err = McpError::internal("Database error");

// Add details
let err = McpError::invalid_params("Bad input")
    .with_detail("field", "name")
    .with_detail("expected", "string");

// Get JSON-RPC code
let code: i32 = err.json_rpc_code();
```

### ErrorKind

```rust
use turbomcp_core::error::ErrorKind;

pub enum ErrorKind {
    // JSON-RPC standard errors
    ParseError,      // -32700
    InvalidRequest,  // -32600
    MethodNotFound,  // -32601
    InvalidParams,   // -32602
    InternalError,   // -32603

    // MCP-specific errors
    ToolNotFound,
    ResourceNotFound,
    PromptNotFound,
    NotInitialized,
    AlreadyInitialized,
    RequestCancelled,

    // Server errors
    Lifecycle,
    Shutdown,
    Middleware,
    Registry,
    Routing,
    ResourceExhausted,
}
```

### Error Constructors

```rust
use turbomcp_core::error::McpError;

// Protocol errors
McpError::parse_error("Invalid JSON");
McpError::invalid_request("Missing method");
McpError::method_not_found("unknown");
McpError::invalid_params("Bad type");
McpError::internal("Server error");

// MCP errors
McpError::tool_not_found("tool_name");
McpError::resource_not_found("uri");
McpError::prompt_not_found("name");
McpError::not_initialized();
McpError::already_initialized();
McpError::request_cancelled();
```

### ServerErrorExt Trait

Additional constructors for server-side errors:

```rust
use turbomcp_core::error::ServerErrorExt;

McpError::lifecycle("Not started");
McpError::shutdown("Timeout");
McpError::middleware("Auth failed");
McpError::registry("Duplicate handler");
McpError::routing("No route");
McpError::resource_exhausted("Pool empty");
```

## Core Types

### Tool

```rust
use turbomcp_core::types::{Tool, ToolInputSchema};

let tool = Tool {
    name: "calculator".to_string(),
    description: Some("Performs calculations".to_string()),
    input_schema: ToolInputSchema::object()
        .property("expression", "string")
        .required(vec!["expression"]),
    annotations: None,
};
```

### Resource

```rust
use turbomcp_core::types::{Resource, ResourceContents};

let resource = Resource {
    uri: "file:///data.json".to_string(),
    name: "data.json".to_string(),
    description: Some("JSON data file".to_string()),
    mime_type: Some("application/json".to_string()),
    annotations: None,
};

let contents = ResourceContents::Text {
    uri: "file:///data.json".to_string(),
    text: r#"{"key": "value"}"#.to_string(),
    mime_type: Some("application/json".to_string()),
};
```

### Prompt

```rust
use turbomcp_core::types::{Prompt, PromptArgument, PromptMessage};

let prompt = Prompt {
    name: "greeting".to_string(),
    description: Some("A greeting prompt".to_string()),
    arguments: Some(vec![
        PromptArgument {
            name: "name".to_string(),
            description: Some("Name to greet".to_string()),
            required: Some(true),
        }
    ]),
};
```

### Content

```rust
use turbomcp_core::types::{Content, TextContent, ImageContent, EmbeddedResource};

// Text content
let text = Content::Text(TextContent {
    text: "Hello, world!".to_string(),
    annotations: None,
});

// Image content
let image = Content::Image(ImageContent {
    data: base64_data.to_string(),
    mime_type: "image/png".to_string(),
    annotations: None,
});

// Embedded resource
let resource = Content::Resource(EmbeddedResource {
    resource: resource_contents,
    annotations: None,
});
```

## Capabilities

### ServerCapabilities

```rust
use turbomcp_core::types::{ServerCapabilities, ToolsCapability, ResourcesCapability};

let capabilities = ServerCapabilities {
    tools: Some(ToolsCapability {
        list_changed: Some(true),
    }),
    resources: Some(ResourcesCapability {
        subscribe: Some(true),
        list_changed: Some(true),
    }),
    prompts: Some(PromptsCapability {
        list_changed: Some(true),
    }),
    logging: Some(LoggingCapability {}),
    experimental: None,
};
```

### ClientCapabilities

```rust
use turbomcp_core::types::{ClientCapabilities, SamplingCapability};

let capabilities = ClientCapabilities {
    sampling: Some(SamplingCapability {}),
    roots: Some(RootsCapability {
        list_changed: Some(true),
    }),
    experimental: None,
};
```

## JSON-RPC Types

### Request

```rust
use turbomcp_core::jsonrpc::{Request, RequestId};

let request = Request {
    jsonrpc: "2.0".to_string(),
    id: RequestId::Number(1),
    method: "tools/call".to_string(),
    params: Some(serde_json::json!({
        "name": "calculator",
        "arguments": {"expression": "2 + 2"}
    })),
};
```

### Response

```rust
use turbomcp_core::jsonrpc::{Response, ResponseError};

// Success response
let response = Response::success(
    RequestId::Number(1),
    serde_json::json!({"result": 4}),
);

// Error response
let response = Response::error(
    RequestId::Number(1),
    ResponseError {
        code: -32602,
        message: "Invalid params".to_string(),
        data: None,
    },
);
```

### Notification

```rust
use turbomcp_core::jsonrpc::Notification;

let notification = Notification {
    jsonrpc: "2.0".to_string(),
    method: "notifications/tools/list_changed".to_string(),
    params: None,
};
```

## Initialization Types

### InitializeRequest

```rust
use turbomcp_core::types::{InitializeRequest, ClientInfo};

let request = InitializeRequest {
    protocol_version: "2025-11-25".to_string(),
    capabilities: client_capabilities,
    client_info: ClientInfo {
        name: "my-client".to_string(),
        version: "1.0.0".to_string(),
    },
};
```

### InitializeResult

```rust
use turbomcp_core::types::{InitializeResult, ServerInfo};

let result = InitializeResult {
    protocol_version: "2025-11-25".to_string(),
    capabilities: server_capabilities,
    server_info: ServerInfo {
        name: "my-server".to_string(),
        version: "1.0.0".to_string(),
    },
    instructions: Some("Welcome to my MCP server".to_string()),
};
```

## no_std Usage

For embedded or WASM environments:

```rust
#![no_std]
extern crate alloc;

use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::types::Tool;
use alloc::string::String;

fn create_tool() -> Tool {
    Tool {
        name: String::from("example"),
        description: None,
        input_schema: Default::default(),
        annotations: None,
    }
}

fn handler() -> McpResult<&'static str> {
    Err(McpError::invalid_params("error"))
}
```

## Serialization

All types implement `Serialize` and `Deserialize`:

```rust
use turbomcp_core::types::Tool;
use serde_json;

let tool = Tool { /* ... */ };

// To JSON
let json = serde_json::to_string(&tool)?;

// From JSON
let parsed: Tool = serde_json::from_str(&json)?;
```

## Type Conversions

### From/Into Implementations

```rust
use turbomcp_core::error::McpError;

// From string errors
let err: McpError = "error message".into();

// From std::io::Error (with std feature)
let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
let mcp_err: McpError = io_err.into();
```

## Constants

### Protocol Version

```rust
use turbomcp_core::PROTOCOL_VERSION;

assert_eq!(PROTOCOL_VERSION, "2025-11-25");
```

### Error Codes

```rust
use turbomcp_core::error::codes;

codes::PARSE_ERROR        // -32700
codes::INVALID_REQUEST    // -32600
codes::METHOD_NOT_FOUND   // -32601
codes::INVALID_PARAMS     // -32602
codes::INTERNAL_ERROR     // -32603
```

## Next Steps

- **[Protocol API](protocol.md)** - Full protocol implementation
- **[Wire Codecs](wire.md)** - Serialization formats
- **[Error Handling Guide](../guide/error-handling.md)** - Error patterns
