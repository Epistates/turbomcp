# MCP Protocol Compliance & Versioning

Comprehensive guide to TurboMCP's implementation of the Model Context Protocol specification and version management.

## Overview

TurboMCP implements the **Model Context Protocol (MCP) specification version 2025-06-18** with full compliance across all defined capabilities. The implementation:

- **Spec Compliant** - 100% adherence to MCP 2025-06-18 specification
- **JSON-RPC 2.0** - Complete JSON-RPC 2.0 protocol implementation
- **Capability Negotiation** - Automatic feature detection and negotiation
- **Version Management** - Backward compatibility and migration support
- **Validation** - Runtime schema validation and type checking
- **Extensibility** - Support for custom extensions and future protocol versions

## Protocol Specification

### MCP Version: 2025-06-18

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

```rust
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

```rust
// Client sends initialize request
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-06-18",
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
    "protocolVersion": "2025-06-18",
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
      "name": "TurboMCP Server",
      "version": "2.1.1"
    }
  },
  "id": 1
}
```

### TurboMCP Implementation

```rust
use turbomcp::prelude::*;

#[server]
pub struct MyServer;

#[tokio::main]
async fn main() -> Result<()> {
    MyServer::new()
        .with_info(ServerInfo {
            name: "MyServer".to_string(),
            version: "1.0.0".to_string(),
        })
        .with_capabilities(|caps| {
            caps
                .with_tools()
                .tool_list_changed(true)  // Support notifications
                .with_resources()
                .resource_subscribe(true)  // Support subscriptions
                .resource_list_changed(true)
                .with_prompts()
                .with_logging()
        })
        .stdio()
        .run()
        .await
}
```

### Capability Builder

Type-state pattern ensures compile-time correctness:

```rust
pub struct CapabilityBuilder<Tools, Resources, Prompts, Logging> {
    _tools: PhantomData<Tools>,
    _resources: PhantomData<Resources>,
    _prompts: PhantomData<Prompts>,
    _logging: PhantomData<Logging>,
    capabilities: ServerCapabilities,
}

// Only available when tools are enabled
impl<R, P, L> CapabilityBuilder<Enabled, R, P, L> {
    pub fn tool_list_changed(mut self, enabled: bool) -> Self {
        self.capabilities.tools.as_mut().unwrap().list_changed = Some(enabled);
        self
    }
}

// Only available when resources are enabled
impl<T, P, L> CapabilityBuilder<T, Enabled, P, L> {
    pub fn resource_subscribe(mut self, enabled: bool) -> Self {
        self.capabilities.resources.as_mut().unwrap().subscribe = Some(enabled);
        self
    }

    pub fn resource_list_changed(mut self, enabled: bool) -> Self {
        self.capabilities.resources.as_mut().unwrap().list_changed = Some(enabled);
        self
    }
}
```

## Tools Capability

### Specification

Tools are executable functions exposed by the server:

- **tools/list** - List available tools
- **tools/call** - Execute a specific tool

### List Tools

```rust
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

```rust
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
#[tool]
#[description("Calculate the sum of an array of numbers")]
pub async fn calculate_sum(
    #[description("Array of numbers to sum")]
    numbers: Vec<f64>,
) -> McpResult<ToolResponse> {
    let sum: f64 = numbers.iter().sum();

    Ok(ToolResponse::text(format!("Sum is: {}", sum)))
}
```

**Schema Generation:**

The `#[tool]` macro automatically generates JSON Schema from Rust types:

```rust
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
  "required": ["numbers"]
}
```

### Tool List Changed Notification

When `tool_list_changed: true` is enabled:

```rust
// Server sends notification
{
  "jsonrpc": "2.0",
  "method": "notifications/tools/list_changed"
}
```

Implementation:

```rust
use turbomcp::notifications::send_tool_list_changed;

#[tool]
pub async fn register_new_tool(
    name: String,
    server: &McpServer,
) -> McpResult<()> {
    // Register new tool dynamically
    server.register_tool(name, /* ... */);

    // Notify clients
    send_tool_list_changed(&server).await?;

    Ok(())
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

```rust
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

```rust
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

```rust
#[resource]
#[uri("file:///documents/{path}")]
#[description("Read files from documents directory")]
#[mime_type("text/plain")]
pub async fn read_file(
    #[description("Relative file path")]
    path: String,
) -> McpResult<ResourceContents> {
    let full_path = format!("/documents/{}", path);
    let contents = tokio::fs::read_to_string(&full_path).await?;

    Ok(ResourceContents::text(contents))
}
```

### Resource Subscriptions

```rust
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

Implementation:

```rust
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct ResourceWatcher {
    updates: broadcast::Sender<String>,
}

impl ResourceWatcher {
    pub async fn watch_file(&self, uri: String) {
        // Watch file for changes
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            if file_changed(&uri).await? {
                // Notify subscribers
                self.updates.send(uri.clone()).ok();
            }
        }
    }
}
```

## Prompts Capability

### Specification

Prompts are reusable templates:

- **prompts/list** - List available prompts
- **prompts/get** - Get prompt with arguments filled

### List Prompts

```rust
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

```rust
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

```rust
#[prompt]
#[description("Review code for best practices")]
pub async fn code_review(
    #[description("Programming language")]
    language: String,
    #[description("Code to review")]
    code: String,
) -> McpResult<PromptResult> {
    Ok(PromptResult {
        description: Some(format!("Code review for {} code", language)),
        messages: vec![
            PromptMessage::user(format!(
                "Please review this {} code:\n\n{}",
                language, code
            )),
        ],
    })
}
```

## Sampling Capability

### Specification

Sampling allows servers to request LLM completions from clients:

- **sampling/createMessage** - Request message generation

### Create Message

```rust
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

```rust
use turbomcp::sampling::{SamplingClient, CreateMessageRequest};

#[tool]
pub async fn translate_with_llm(
    text: String,
    target_lang: String,
    sampling: SamplingClient,
) -> McpResult<String> {
    let request = CreateMessageRequest {
        messages: vec![
            Message::user(format!("Translate '{}' to {}", text, target_lang))
        ],
        max_tokens: 100,
        ..Default::default()
    };

    let response = sampling.create_message(request).await?;

    Ok(response.content.text)
}
```

## Error Codes

TurboMCP implements all JSON-RPC 2.0 standard error codes plus MCP-specific codes:

```rust
pub enum ErrorCode {
    // JSON-RPC 2.0 standard errors
    ParseError = -32700,          // Invalid JSON
    InvalidRequest = -32600,      // Invalid Request object
    MethodNotFound = -32601,      // Method not found
    InvalidParams = -32602,       // Invalid parameters
    InternalError = -32603,       // Internal error

    // MCP-specific errors
    ResourceNotFound = -32001,    // Resource URI not found
    ToolNotFound = -32002,        // Tool name not found
    PromptNotFound = -32003,      // Prompt name not found
    Unauthorized = -32004,        // Authentication required
    RateLimitExceeded = -32005,   // Rate limit exceeded
}

impl McpError {
    pub fn code(&self) -> i32 {
        match self {
            McpError::ParseError(_) => -32700,
            McpError::InvalidRequest(_) => -32600,
            McpError::MethodNotFound(_) => -32601,
            McpError::InvalidParams(_) => -32602,
            McpError::InternalError(_) => -32603,
            McpError::ResourceNotFound(_) => -32001,
            McpError::ToolNotFound(_) => -32002,
            McpError::PromptNotFound(_) => -32003,
            McpError::Unauthorized(_) => -32004,
            McpError::RateLimitExceeded(_) => -32005,
        }
    }
}
```

## Schema Validation

### JSON Schema Generation

TurboMCP uses the `schemars` crate for automatic schema generation:

```rust
use schemars::{JsonSchema, schema_for};

#[derive(Deserialize, JsonSchema)]
pub struct CalculateParams {
    #[schemars(description = "First number")]
    a: f64,
    #[schemars(description = "Second number")]
    b: f64,
}

// Generate schema at compile time
let schema = schema_for!(CalculateParams);
```

### Runtime Validation

```rust
use jsonschema::JSONSchema;

pub fn validate_params(
    params: &Value,
    schema: &Value,
) -> McpResult<()> {
    let compiled = JSONSchema::compile(schema)
        .map_err(|e| McpError::InternalError(e.to_string()))?;

    if let Err(errors) = compiled.validate(params) {
        let error_messages: Vec<String> = errors
            .map(|e| e.to_string())
            .collect();

        return Err(McpError::InvalidParams(
            error_messages.join(", ")
        ));
    }

    Ok(())
}
```

## Version Management

### Protocol Version Detection

```rust
pub struct ProtocolVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl ProtocolVersion {
    pub fn from_string(s: &str) -> Result<Self> {
        // Parse "2025-06-18" format
        let parts: Vec<&str> = s.split('-').collect();
        Ok(Self {
            major: parts[0].parse()?,
            minor: parts[1].parse()?,
            patch: parts[2].parse()?,
        })
    }

    pub fn is_compatible(&self, other: &Self) -> bool {
        // Same major version = compatible
        self.major == other.major
    }
}
```

### Backward Compatibility

```rust
pub struct SessionState {
    protocol_version: ProtocolVersion,
}

impl SessionState {
    pub fn supports_feature(&self, feature: &str) -> bool {
        match feature {
            "tool_list_changed" => self.protocol_version.minor >= 6,
            "resource_subscriptions" => self.protocol_version.minor >= 6,
            "sampling" => self.protocol_version.minor >= 6,
            _ => false,
        }
    }
}
```

### Migration Guide

When upgrading protocol versions:

```rust
// v2025-06-18 -> v2025-12-18 migration example
pub fn migrate_tool_schema(
    old_schema: Value,
    from_version: ProtocolVersion,
) -> Value {
    if from_version.minor < 12 {
        // Add new required fields
        let mut schema = old_schema;
        schema["properties"]["metadata"] = json!({
            "type": "object",
            "description": "Tool metadata (added in 2025-12-18)"
        });
        schema
    } else {
        old_schema
    }
}
```

## Compliance Testing

### Protocol Test Suite

```rust
#[cfg(test)]
mod compliance_tests {
    use super::*;

    #[tokio::test]
    async fn test_initialize_handshake() {
        let server = TestServer::new();

        let request = json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "TestClient",
                    "version": "1.0.0"
                }
            },
            "id": 1
        });

        let response = server.handle_request(request).await.unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response["result"]["protocolVersion"] == "2025-06-18");
        assert!(response["id"] == 1);
    }

    #[tokio::test]
    async fn test_tools_list() {
        let server = TestServer::with_tool("test_tool");

        let request = json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": 2
        });

        let response = server.handle_request(request).await.unwrap();

        let tools = &response["result"]["tools"];
        assert!(tools.is_array());
        assert_eq!(tools[0]["name"], "test_tool");
    }

    #[tokio::test]
    async fn test_error_codes() {
        let server = TestServer::new();

        // Method not found
        let request = json!({
            "jsonrpc": "2.0",
            "method": "nonexistent/method",
            "id": 3
        });

        let response = server.handle_request(request).await.unwrap();
        assert_eq!(response["error"]["code"], -32601);

        // Invalid params
        let request = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {},  // Missing required fields
            "id": 4
        });

        let response = server.handle_request(request).await.unwrap();
        assert_eq!(response["error"]["code"], -32602);
    }
}
```

### Fuzzing

```rust
#[cfg(fuzzing)]
pub fn fuzz_protocol_parsing(data: &[u8]) {
    if let Ok(json) = serde_json::from_slice::<Value>(data) {
        let _ = parse_json_rpc_request(&json);
    }
}
```

## Best Practices

### 1. Always Validate Protocol Version

```rust
pub async fn handle_initialize(
    request: InitializeRequest,
) -> McpResult<InitializeResponse> {
    let version = ProtocolVersion::from_string(&request.protocol_version)?;

    if !SUPPORTED_VERSION.is_compatible(&version) {
        return Err(McpError::UnsupportedProtocol(
            format!("Unsupported protocol version: {}", request.protocol_version)
        ));
    }

    // Proceed with initialization
    Ok(/* ... */)
}
```

### 2. Implement Graceful Degradation

```rust
pub fn negotiate_capabilities(
    client_caps: ClientCapabilities,
    server_caps: ServerCapabilities,
) -> NegotiatedCapabilities {
    NegotiatedCapabilities {
        tools: client_caps.tools.is_some() && server_caps.tools.is_some(),
        resources: client_caps.resources.is_some() && server_caps.resources.is_some(),
        sampling: client_caps.sampling.is_some() && server_caps.sampling.is_some(),
        // ...
    }
}
```

### 3. Document Protocol Extensions

```rust
/// Custom extension for analytics tracking.
///
/// **Protocol Extension:** `x-analytics`
/// **Supported Since:** TurboMCP v2.1.0
/// **MCP Spec Version:** 2025-06-18
///
/// This extension adds analytics metadata to tool responses.
#[tool]
pub async fn tracked_tool(
    input: String,
) -> McpResult<ToolResponseWithAnalytics> {
    Ok(ToolResponseWithAnalytics {
        content: vec![/* ... */],
        analytics: AnalyticsMetadata {
            tracked: true,
            session_id: "session-123",
        },
    })
}
```

### 4. Test Against Multiple Protocol Versions

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_protocol_2025_06_18() {
        test_with_version("2025-06-18");
    }

    #[test]
    fn test_protocol_2025_12_18() {
        test_with_version("2025-12-18");
    }

    fn test_with_version(version: &str) {
        let server = TestServer::with_version(version);
        // Run compliance tests
    }
}
```

## Related Documentation

- [System Design](./system-design.md) - Architecture overview
- [Context Lifecycle](./context-lifecycle.md) - Request flow
- [Dependency Injection](./dependency-injection.md) - DI system
- [MCP Specification](https://spec.modelcontextprotocol.io/2025-06-18/) - Official spec
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification) - JSON-RPC spec
