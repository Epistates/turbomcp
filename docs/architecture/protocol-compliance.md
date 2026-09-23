# MCP Protocol Compliance & Versioning

Comprehensive guide to TurboMCP's implementation of the Model Context Protocol specification and version management.

## Overview

TurboMCP implements the **Model Context Protocol (MCP)** specification, serving
versions **2025-11-25** (preferred) and **2025-06-18**. The implementation:

- **Spec Compliant** - Tracks the MCP 2025-11-25 specification, with per-version response adapters for 2025-06-18
- **JSON-RPC 2.0** - Complete JSON-RPC 2.0 protocol implementation
- **Capability Negotiation** - Capabilities derived from what the server actually implements
- **Version Management** - Multi-version negotiation; an unsupported request is answered with the preferred version, as the lifecycle spec requires
- **Validation** - Runtime schema validation and type checking
- **Extensibility** - Support for custom extensions within the current protocol surface

## Protocol Specification

### MCP Version: 2025-11-25

The Model Context Protocol defines how Large Language Models (LLMs) interact with external context providers (MCP servers). TurboMCP provides a complete server-side implementation.

**Key Protocol Features:**

- Tools - Executable functions with typed parameters
- Resources - Static or dynamic content sources
- Prompts - Template prompts with placeholders
- Sampling - LLM sampling requests from server to client
- Elicitation - Server-initiated requests for user input
- Notifications - Asynchronous event notifications

### JSON-RPC 2.0 Foundation

MCP is built on JSON-RPC 2.0, which TurboMCP implements fully:

```jsonc
// JSON-RPC 2.0 Request
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "calculate_sum",
    "arguments": {
      "numbers": [1, 2, 3, 4, 5]
    }
  },
  "id": "req-123"
}

// JSON-RPC 2.0 Response (Success)
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Sum is: 15"
      }
    ]
  },
  "id": "req-123"
}

// JSON-RPC 2.0 Response (Error)
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32602,
    "message": "Invalid parameters",
    "data": {
      "details": "Missing required field 'numbers'"
    }
  },
  "id": "req-123"
}

// JSON-RPC 2.0 Notification (no id field)
{
  "jsonrpc": "2.0",
  "method": "notifications/tools/list_changed",
  "params": {}
}
```

## Capability Negotiation

### Initialize Handshake

Every MCP session begins with an `initialize` request:

```jsonc
// Client sends initialize request
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-11-25",
    "capabilities": {
      "sampling": {}
    },
    "clientInfo": {
      "name": "ExampleClient",
      "version": "1.0.0"
    }
  },
  "id": 1
}

// Server responds with capabilities
{
  "jsonrpc": "2.0",
  "result": {
    "protocolVersion": "2025-11-25",
    "capabilities": {
      "tools": {
        "listChanged": true
      },
      "resources": {
        "subscribe": true,
        "listChanged": true
      },
      "prompts": {
        "listChanged": false
      },
      "logging": {}
    },
    "serverInfo": {
      "name": "my-server",
      "version": "1.0.0"
    }
  },
  "id": 1
}
```

### TurboMCP Implementation

`#[server]` answers `initialize` for you. It sends the name and version from the
attribute and advertises exactly the capabilities the impl block serves:
`tools`, `resources`, and `prompts` (each with `listChanged: true`) when it has
handlers of that kind, `resources.subscribe` for a `#[subscribe]` handler,
`completions` for `#[completion]`, and `logging` always.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_stdio().await
}
```

Protocol version negotiation is configured with `ProtocolConfig`: the default
serves `2025-06-18` and `2025-11-25` and prefers the latter.

### Capability Builder

For a hand-written `McpHandler` or protocol-level code, `turbomcp-protocol` has
type-state builders where a sub-capability is only available once its parent is
enabled:

```rust
use turbomcp_protocol::capabilities::builders::ServerCapabilitiesBuilder;

let capabilities = ServerCapabilitiesBuilder::new()
    .enable_tools()
    .enable_tool_list_changed()    // only compiles after enable_tools()
    .enable_resources()
    .enable_resources_subscribe()  // only compiles after enable_resources()
    .enable_prompts()
    .enable_logging()
    .build();
```

## Tools Capability

### Specification

Tools are executable functions exposed by the server:

- **tools/list** - List available tools
- **tools/call** - Execute a specific tool

### List Tools

```jsonc
// Request
{
  "jsonrpc": "2.0",
  "method": "tools/list",
  "id": 2
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "tools": [
      {
        "name": "calculate_sum",
        "description": "Calculate the sum of an array of numbers",
        "inputSchema": {
          "type": "object",
          "properties": {
            "numbers": {
              "type": "array",
              "items": { "type": "number" },
              "description": "Array of numbers to sum"
            }
          },
          "required": ["numbers"]
        }
      }
    ]
  },
  "id": 2
}
```

### Call Tool

```jsonc
// Request
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "calculate_sum",
    "arguments": {
      "numbers": [1, 2, 3, 4, 5]
    }
  },
  "id": 3
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Sum is: 15"
      }
    ],
    "isError": false
  },
  "id": 3
}
```

### TurboMCP Implementation

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct MathServer;

#[server(name = "math", version = "1.0.0")]
impl MathServer {
    /// Calculate the sum of an array of numbers
    #[tool]
    async fn calculate_sum(
        &self,
        #[description("Array of numbers to sum")]
        numbers: Vec<f64>,
    ) -> McpResult<String> {
        let sum: f64 = numbers.iter().sum();
        Ok(format!("Sum is: {}", sum))
    }
}
```

**Schema Generation:**

The `#[tool]` macro automatically generates JSON Schema from Rust types:

```jsonc
// Generated schema
{
  "type": "object",
  "properties": {
    "numbers": {
      "type": "array",
      "items": { "type": "number" },
      "description": "Array of numbers to sum"
    }
  },
  "required": ["numbers"],
  "additionalProperties": false
}
```

### Tool List Changed Notification

Every `#[server]` with tools advertises `tools.listChanged`:

```jsonc
// Server sends notification
{
  "jsonrpc": "2.0",
  "method": "notifications/tools/list_changed"
}
```

A `#[server]` type's tool list is fixed at compile time, but what a client sees
can change: a `VisibilityLayer` session override, or a hand-written
`McpHandler` with a dynamic catalogue. Send the notification from any handler
with the request context:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use turbomcp::prelude::*;

#[derive(Clone, Default)]
pub struct Toggle {
    advanced: Arc<AtomicBool>,
}

#[server(name = "toggle", version = "1.0.0")]
impl Toggle {
    /// Show the advanced tools (a filtering layer reads the flag)
    #[tool]
    async fn enable_advanced(&self, ctx: &RequestContext) -> McpResult<String> {
        self.advanced.store(true, Ordering::Relaxed);
        // Tell this client to re-list
        ctx.notify_tools_list_changed().await?;
        Ok("advanced tools enabled".to_string())
    }
}
```

## Resources Capability

### Specification

Resources provide access to data sources:

- **resources/list** - List available resources
- **resources/read** - Read resource contents
- **resources/subscribe** - Subscribe to resource updates
- **resources/unsubscribe** - Unsubscribe from updates

### List Resources

```jsonc
// Request
{
  "jsonrpc": "2.0",
  "method": "resources/list",
  "id": 4
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "resources": [
      {
        "uri": "file:///documents/readme.md",
        "name": "README",
        "description": "Project README file",
        "mimeType": "text/markdown"
      }
    ]
  },
  "id": 4
}
```

### Read Resource

```jsonc
// Request
{
  "jsonrpc": "2.0",
  "method": "resources/read",
  "params": {
    "uri": "file:///documents/readme.md"
  },
  "id": 5
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "contents": [
      {
        "uri": "file:///documents/readme.md",
        "mimeType": "text/markdown",
        "text": "# My Project\n\nWelcome to my project..."
      }
    ]
  },
  "id": 5
}
```

### TurboMCP Implementation

The URI (or RFC 6570 template) is the attribute's first argument. The handler
receives the full requested URI and the request context:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Documents;

#[server(name = "documents", version = "1.0.0")]
impl Documents {
    /// Read files from the documents directory
    #[resource("file:///documents/{path}", mime_type = "text/plain")]
    async fn read_file(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        let path = uri.trim_start_matches("file:///documents/");
        if path.split('/').any(|segment| segment == "..") {
            return Err(McpError::invalid_params("path escapes the documents directory"));
        }
        tokio::fs::read_to_string(format!("/documents/{path}"))
            .await
            .map_err(|_| McpError::resource_not_found(&uri))
    }
}
```

### Resource Subscriptions

```jsonc
// Subscribe request
{
  "jsonrpc": "2.0",
  "method": "resources/subscribe",
  "params": {
    "uri": "file:///documents/log.txt"
  },
  "id": 6
}

// Update notification
{
  "jsonrpc": "2.0",
  "method": "notifications/resources/updated",
  "params": {
    "uri": "file:///documents/log.txt"
  }
}
```

Implementation: declare `#[subscribe]` and `#[unsubscribe]` (together; one
without the other is a compile error), which advertises
`resources.subscribe`, and send updates with `ctx.notify_resource_updated`:

```rust
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use turbomcp::prelude::*;

#[derive(Clone, Default)]
pub struct Logs {
    subscribed: Arc<Mutex<HashSet<String>>>,
    lines: Arc<Mutex<Vec<String>>>,
}

#[server(name = "logs", version = "1.0.0")]
impl Logs {
    /// The log file
    #[resource("file:///documents/log.txt", mime_type = "text/plain")]
    async fn log(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(self.lines.lock().unwrap().join("\n"))
    }

    #[subscribe]
    async fn subscribe(&self, uri: String) -> McpResult<()> {
        self.subscribed.lock().unwrap().insert(uri);
        Ok(())
    }

    #[unsubscribe]
    async fn unsubscribe(&self, uri: String) -> McpResult<()> {
        self.subscribed.lock().unwrap().remove(&uri);
        Ok(())
    }

    /// Append a line, and tell a subscribed client the resource changed
    #[tool]
    async fn append(&self, line: String, ctx: &RequestContext) -> McpResult<String> {
        self.lines.lock().unwrap().push(line);
        let uri = "file:///documents/log.txt";
        let is_subscribed = self.subscribed.lock().unwrap().contains(uri);
        if is_subscribed {
            ctx.notify_resource_updated(uri).await?;
        }
        Ok("appended".to_string())
    }
}
```

## Prompts Capability

### Specification

Prompts are reusable templates:

- **prompts/list** - List available prompts
- **prompts/get** - Get prompt with arguments filled

### List Prompts

```jsonc
// Request
{
  "jsonrpc": "2.0",
  "method": "prompts/list",
  "id": 7
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "prompts": [
      {
        "name": "code_review",
        "description": "Review code for best practices",
        "arguments": [
          {
            "name": "language",
            "description": "Programming language",
            "required": true
          },
          {
            "name": "code",
            "description": "Code to review",
            "required": true
          }
        ]
      }
    ]
  },
  "id": 7
}
```

### Get Prompt

```jsonc
// Request
{
  "jsonrpc": "2.0",
  "method": "prompts/get",
  "params": {
    "name": "code_review",
    "arguments": {
      "language": "Rust",
      "code": "fn main() { println!(\"Hello\"); }"
    }
  },
  "id": 8
}

// Response
{
  "jsonrpc": "2.0",
  "result": {
    "description": "Code review for Rust code",
    "messages": [
      {
        "role": "user",
        "content": {
          "type": "text",
          "text": "Please review this Rust code:\n\nfn main() { println!(\"Hello\"); }"
        }
      }
    ]
  },
  "id": 8
}
```

### TurboMCP Implementation

Prompt arguments are `String` (required) or `Option<String>` (optional); the
handler takes the request context last:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Reviews;

#[server(name = "reviews", version = "1.0.0")]
impl Reviews {
    /// Review code for best practices
    #[prompt]
    async fn code_review(
        &self,
        #[description("Programming language")] language: String,
        #[description("Code to review")] code: String,
        ctx: &RequestContext,
    ) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!(
            "Please review this {} code:\n\n{}",
            language, code
        ))
        .with_description(format!("Code review for {} code", language)))
    }
}
```

## Sampling Capability

### Specification

Sampling allows servers to request LLM completions from clients:

- **sampling/createMessage** - Request message generation

### Create Message

```jsonc
// Server -> Client request
{
  "jsonrpc": "2.0",
  "method": "sampling/createMessage",
  "params": {
    "messages": [
      {
        "role": "user",
        "content": {
          "type": "text",
          "text": "Translate 'hello' to French"
        }
      }
    ],
    "maxTokens": 100
  },
  "id": "sampling-1"
}

// Client -> Server response
{
  "jsonrpc": "2.0",
  "result": {
    "role": "assistant",
    "content": {
      "type": "text",
      "text": "Bonjour"
    },
    "model": "claude-3-opus",
    "stopReason": "end_turn"
  },
  "id": "sampling-1"
}
```

### TurboMCP Implementation

A handler asks the client for a completion with `ctx.sample`. The client must
have declared the `sampling` capability. The request types come from
`turbomcp-types`:

```rust
use turbomcp::prelude::*;
use turbomcp_types::{CreateMessageRequest, SamplingMessage};

#[derive(Clone)]
pub struct Translator;

#[server(name = "translator", version = "1.0.0")]
impl Translator {
    /// Translate text with the client's model
    #[tool]
    async fn translate_with_llm(
        &self,
        text: String,
        target_lang: String,
        ctx: &RequestContext,
    ) -> McpResult<String> {
        let request = CreateMessageRequest {
            messages: vec![SamplingMessage::user(format!(
                "Translate '{}' to {}",
                text, target_lang
            ))],
            max_tokens: 100,
            ..Default::default()
        };

        let response = ctx.sample(request).await?;
        Ok(response.content.as_text().unwrap_or_default().to_string())
    }
}
```

## Error Codes

`turbomcp_core::error_codes` defines the codes TurboMCP puts on the wire, and
each `McpError` constructor picks the right one:

```rust
use turbomcp_core::error_codes;

fn main() {
// JSON-RPC 2.0 standard errors
assert_eq!(error_codes::PARSE_ERROR, -32700);      // Invalid JSON
assert_eq!(error_codes::INVALID_REQUEST, -32600);  // Invalid Request object
assert_eq!(error_codes::METHOD_NOT_FOUND, -32601); // Method not found
assert_eq!(error_codes::INVALID_PARAMS, -32602);   // Invalid parameters
assert_eq!(error_codes::INTERNAL_ERROR, -32603);   // Internal error

// Codes the MCP specification assigns
assert_eq!(error_codes::RESOURCE_NOT_FOUND, -32002);
assert_eq!(error_codes::URL_ELICITATION_REQUIRED, -32042);

// Spelled out by the MCP spec in terms of the standard codes
assert_eq!(error_codes::TOOL_NOT_FOUND, -32602);           // "Unknown tool"
assert_eq!(error_codes::PROMPT_NOT_FOUND, -32602);         // "Invalid prompt name"
assert_eq!(error_codes::CAPABILITY_NOT_SUPPORTED, -32601); // unsupported optional method
}
```

A tool that runs and fails is not a protocol error: it is a successful
`tools/call` response with `isError: true`, with the error kind in `_meta`.

## Schema Validation

### JSON Schema Generation

TurboMCP uses the `schemars` crate for automatic schema generation:

```rust
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct CalculateParams {
    /// First number
    a: f64,
    /// Second number
    b: f64,
}

fn main() {
    // The same generator the macros use for a parameter of this type
    let schema = schema_for!(CalculateParams);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
```

### Runtime Validation

The dispatcher generated by `#[server]` validates `tools/call` arguments
against the tool's parameters: an argument the tool does not declare, a
missing required one, or one of the wrong type is reported as a tool execution
error classified `invalid_params`. No separate JSON Schema validator runs.

## Version Management

### Protocol Version Negotiation

MCP versions are dates, compared as whole strings. `ProtocolConfig` controls
what the server speaks:

```rust
use turbomcp::prelude::*;
use turbomcp_server::ProtocolVersion;

fn main() {
    // Default: supports 2025-06-18 and 2025-11-25, prefers 2025-11-25
    let config = ProtocolConfig::default();
    assert_eq!(config.negotiate(Some("2025-06-18")), Some(ProtocolVersion::V2025_06_18));

    // A version the server does not support is answered with the preferred one,
    // as the lifecycle spec requires; the client decides whether to continue
    assert_eq!(config.negotiate(Some("2024-11-05")), Some(ProtocolVersion::LATEST));

    // Speak only one version (still offered to clients that asked for another)
    let strict = ProtocolConfig::strict(ProtocolVersion::LATEST);
    assert_eq!(strict.negotiate(Some("2025-06-18")), Some(ProtocolVersion::LATEST));
}
```

Pass it with `builder().with_protocol(config)`.

### Backward Compatibility

A client that negotiated `2025-06-18` gets responses through that version's
adapter: fields added in `2025-11-25` (icons, tool `execution`, and the like)
are left off the wire rather than sent to a client that does not know them.

## Compliance Testing

### Protocol Test Suite

`McpHandlerExt::handle_request` runs one JSON-RPC request through the same
router the transports use, which makes wire-level tests short:

```rust
use serde_json::json;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct TestServer;

#[server(name = "test-server", version = "1.0.0")]
impl TestServer {
    /// A tool for the tests
    #[tool]
    async fn test_tool(&self) -> String {
        "ok".to_string()
    }
}

#[cfg(test)]
mod compliance_tests {
    use super::*;

    async fn request(body: serde_json::Value) -> serde_json::Value {
        TestServer.handle_request(body, RequestContext::new()).await.unwrap()
    }

    #[tokio::test]
    async fn test_initialize_handshake() {
        let response = request(json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "TestClient", "version": "1.0.0" }
            },
            "id": 1
        }))
        .await;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(response["id"], 1);
    }

    #[tokio::test]
    async fn test_tools_list() {
        let response = request(json!({ "jsonrpc": "2.0", "method": "tools/list", "id": 2 })).await;
        assert_eq!(response["result"]["tools"][0]["name"], "test_tool");
    }

    #[tokio::test]
    async fn test_error_codes() {
        // Method not found
        let response =
            request(json!({ "jsonrpc": "2.0", "method": "nonexistent/method", "id": 3 })).await;
        assert_eq!(response["error"]["code"], -32601);

        // Unknown tool
        let response = request(json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "no_such_tool" },
            "id": 4
        }))
        .await;
        assert_eq!(response["error"]["code"], -32602);
    }
}
```

### Fuzzing

`crates/turbomcp-protocol/fuzz` has `cargo-fuzz` targets for JSON-RPC parsing,
message validation, capability parsing, and tool deserialization. A target
looks like this (not compiled here: it needs the `cargo fuzz` harness):

```rust,ignore
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = turbomcp_server::parse_request(input);
    }
});
```

## Best Practices

### 1. Leave Version Negotiation to the Framework

The default `ProtocolConfig` already implements the lifecycle rules: answer a
supported version with itself and anything else with the preferred version.
Setting `allow_fallback = false` makes the server refuse the handshake with
`-32602` instead, which the spec does not permit.

### 2. Check Client Capabilities Before Server-to-Client Requests

`ctx.sample`, `ctx.elicit_form`, and `ctx.list_roots` fail with
`capability_not_supported` when the client did not declare the capability.
Handle that error and degrade, rather than failing the whole tool call.

### 3. Document Protocol Extensions

Put non-standard metadata in `_meta` under a reverse-DNS key, as TurboMCP does
with `io.turbomcp/tags` and `io.turbomcp/errorKind`, so clients that do not
know it can ignore it.

### 4. Test Against Multiple Protocol Versions

Run the handshake test above with `"protocolVersion": "2025-06-18"` as well,
and assert on the fields that version's clients must not receive.

## Related Documentation

- [System Design](./system-design.md) - Architecture overview
- [Context Lifecycle](./context-lifecycle.md) - Request flow
- [Dependency Injection](./dependency-injection.md) - Handler parameters and shared state
- [MCP Specification](https://spec.modelcontextprotocol.io/2025-11-25/) - Official spec
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification) - JSON-RPC spec
