# Core API Reference

The `turbomcp-core` crate is the portable foundation layer for TurboMCP. It
contains error handling, JSON-RPC envelopes, handler/context traits, security
helpers, and common MCP definition/result re-exports from `turbomcp-types`.

Complete protocol request/result/capability types live in `turbomcp-types` and
are also re-exported by `turbomcp-protocol` for async protocol users.

## Installation

```toml
[dependencies]
turbomcp-core = "3.2.0"

# For no_std environments
turbomcp-core = { version = "3.2.0", default-features = false }
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Standard library support | Yes |
| `rich-errors` | UUID/timestamp error tracking | No |
| `wasm` | WASM-specific optimizations | No |
| `zero-copy` | rkyv-backed internal message types | No |

## Errors

```rust
use turbomcp_core::{ErrorKind, McpError, McpResult};

let err = McpError::new(ErrorKind::InvalidParams, "missing field");
let err = McpError::tool_not_found("calculator");
let err = McpError::internal("database error");

let err = McpError::invalid_params("bad input")
    .with_operation("tools/call")
    .with_component("tool_registry")
    .with_request_id("42");

assert_eq!(err.jsonrpc_code(), -32602);
```

Common constructors:

```rust
use turbomcp_core::McpError;

McpError::parse_error("invalid JSON");
McpError::invalid_request("missing method");
McpError::method_not_found("unknown");
McpError::invalid_params("bad type");
McpError::internal("server error");
McpError::tool_not_found("tool_name");
McpError::resource_not_found("file:///missing");
McpError::prompt_not_found("prompt_name");
McpError::timeout("deadline exceeded");
McpError::transport("connection closed");
McpError::cancelled("client cancelled request");
```

## Common Type Re-Exports

`turbomcp-core` re-exports the most commonly used definition and result types at
the crate root:

```rust
use turbomcp_core::{Tool, ToolInputSchema};

let tool = Tool::new("calculator", "Performs calculations").with_schema(
    ToolInputSchema::default()
        .add_property("expression", serde_json::json!({"type": "string"}))
        .require_property("expression"),
);
```

```rust
use turbomcp_core::{Resource, ResourceContents};
use turbomcp_types::TextResourceContents;

let resource = Resource::new("file:///data.json", "data.json")
    .with_description("JSON data file")
    .with_mime_type("application/json");

let contents = ResourceContents::Text(TextResourceContents {
    uri: resource.uri.clone(),
    mime_type: Some("application/json".to_string()),
    text: r#"{"key":"value"}"#.to_string(),
    meta: None,
});
```

```rust
use turbomcp_core::{Prompt, PromptArgument};

let prompt = Prompt::new("greeting", "A greeting prompt")
    .with_argument(PromptArgument::required("name", "Name to greet"));
```

```rust
use turbomcp_core::Content;

let text = Content::text("Hello, world!");
let image = Content::image(base64_data, "image/png");
let embedded = Content::resource("file:///example.txt", "file contents");
```

## Capabilities And Wire Types

Capabilities and initialization wire wrappers are owned by `turbomcp-types`:

```rust
use turbomcp_types::{
    ClientCapabilities, Implementation, InitializeRequest, InitializeResult, LoggingCapabilities,
    PromptsCapabilities, ProtocolVersion, ResourcesCapabilities, RootsCapabilities,
    SamplingCapabilities, ServerCapabilities, ToolsCapabilities,
};

let server_capabilities = ServerCapabilities {
    tools: Some(ToolsCapabilities {
        list_changed: Some(true),
    }),
    resources: Some(ResourcesCapabilities {
        subscribe: Some(true),
        list_changed: Some(true),
    }),
    prompts: Some(PromptsCapabilities {
        list_changed: Some(true),
    }),
    logging: Some(LoggingCapabilities {}),
    ..Default::default()
};

let client_capabilities = ClientCapabilities {
    sampling: Some(SamplingCapabilities::default()),
    roots: Some(RootsCapabilities {
        list_changed: Some(true),
    }),
    ..Default::default()
};

let request = InitializeRequest {
    protocol_version: ProtocolVersion::LATEST,
    capabilities: client_capabilities,
    client_info: Implementation::new("my-client", "1.0.0"),
    meta: None,
};

let result = InitializeResult {
    protocol_version: ProtocolVersion::LATEST,
    capabilities: server_capabilities,
    server_info: Implementation::new("my-server", "1.0.0"),
    instructions: Some("Welcome to my MCP server".to_string()),
    meta: None,
};
```

## JSON-RPC Envelopes

```rust
use turbomcp_core::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

let request = JsonRpcRequest::new(
    "tools/call",
    Some(serde_json::json!({
        "name": "calculator",
        "arguments": {"expression": "2 + 2"}
    })),
    1,
);

let success = JsonRpcResponse::success(serde_json::json!({"result": 4}), request.id.clone());

let error = JsonRpcResponse::error_response(
    JsonRpcError::invalid_params("expression must be a string"),
    request.id,
);

let notification = JsonRpcNotification::without_params("notifications/tools/list_changed");
```

## no_std Usage

```rust
#![no_std]
extern crate alloc;

use alloc::string::String;
use turbomcp_core::{McpError, McpResult, Tool};

fn create_tool() -> Tool {
    Tool::new(String::from("example"), String::from("Example tool"))
}

fn handler() -> McpResult<&'static str> {
    Err(McpError::invalid_params("error"))
}
```

## Constants

```rust
use turbomcp_core::{JSONRPC_VERSION, PROTOCOL_VERSION};

assert_eq!(JSONRPC_VERSION, "2.0");
assert_eq!(PROTOCOL_VERSION, "2025-11-25");
```

## Next Steps

- [Protocol API](protocol.md) - Full protocol implementation
- [Wire Codecs](wire.md) - Serialization formats
- [Error Handling Guide](../guide/error-handling.md) - Error patterns
