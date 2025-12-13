# Client API Reference

Complete API reference for building MCP clients with TurboMCP.

## Overview

The TurboMCP client API provides a complete implementation for connecting to MCP servers, calling tools, reading resources, and managing prompts. The client handles connection management, request correlation, error recovery, and capability negotiation.

## Core Types

### Client

The main client type for interacting with MCP servers.

```rust
use turbomcp_client::prelude::*;

let transport = StdioTransport::new();
let client = Client::new(transport);
```

#### Creating a Client

```rust
// STDIO transport (for local processes)
let transport = StdioTransport::new();
let client = Client::new(transport);

// HTTP transport
let transport = HttpTransport::new("http://localhost:8080");
let client = Client::new(transport);

// WebSocket transport
let transport = WebSocketTransport::connect("ws://localhost:8081").await?;
let client = Client::new(transport);

// TCP transport
let transport = TcpTransport::connect("localhost:9000").await?;
let client = Client::new(transport);
```

## Connection Management

### Initialization

Initialize the connection and perform capability negotiation:

```rust
let init_result = client.initialize().await?;

println!("Server: {} v{}",
    init_result.server_info.name,
    init_result.server_info.version
);

// Check server capabilities
if init_result.capabilities.tools.is_some() {
    println!("Server supports tools");
}
if init_result.capabilities.resources.is_some() {
    println!("Server supports resources");
}
if init_result.capabilities.prompts.is_some() {
    println!("Server supports prompts");
}
```

#### InitializeResult

```rust
pub struct InitializeResult {
    pub protocol_version: String,
    pub server_info: ServerInfo,
    pub capabilities: ServerCapabilities,
}

pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

pub struct ServerCapabilities {
    pub tools: Option<ToolsCapability>,
    pub resources: Option<ResourcesCapability>,
    pub prompts: Option<PromptsCapability>,
    pub logging: Option<LoggingCapability>,
}
```

### Connection State

Check and manage connection state:

```rust
// Check if connected
if client.is_connected() {
    println!("Client is connected");
}

// Wait for connection
client.wait_for_connection().await?;

// Disconnect
client.disconnect().await?;
```

## Tool Operations

### Listing Tools

Get all available tools from the server:

```rust
let tools = client.list_tools().await?;

for tool in tools {
    println!("Tool: {}", tool.name);
    if let Some(desc) = tool.description {
        println!("  Description: {}", desc);
    }
    if let Some(schema) = tool.input_schema {
        println!("  Schema: {}", serde_json::to_string_pretty(&schema)?);
    }
}
```

#### ToolInfo

```rust
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}
```

### Calling Tools

Execute a tool on the server:

```rust
use std::collections::HashMap;

// Build arguments
let mut args = HashMap::new();
args.insert("city".to_string(), serde_json::json!("San Francisco"));
args.insert("units".to_string(), serde_json::json!("metric"));

// Call tool
let result = client.call_tool("get_weather", Some(args)).await?;

// Parse result
match result.content {
    ToolCallContent::Text { text } => {
        println!("Result: {}", text);
    }
    ToolCallContent::Image { data, mime_type } => {
        println!("Got image: {} ({} bytes)", mime_type, data.len());
    }
    ToolCallContent::Resource { uri, text, blob } => {
        println!("Got resource: {}", uri);
    }
}
```

#### ToolCallResult

```rust
pub struct ToolCallResult {
    pub content: ToolCallContent,
    pub is_error: bool,
}

pub enum ToolCallContent {
    Text {
        text: String,
    },
    Image {
        data: Vec<u8>,
        mime_type: String,
    },
    Resource {
        uri: String,
        text: Option<String>,
        blob: Option<Vec<u8>>,
    },
}
```

### Tool Call Options

Configure tool call behavior:

```rust
let options = ToolCallOptions {
    timeout: Some(Duration::from_secs(30)),
    retry_count: Some(3),
    retry_delay: Some(Duration::from_millis(100)),
};

let result = client
    .call_tool_with_options("long_operation", None, options)
    .await?;
```

## Resource Operations

### Listing Resources

Get all available resources:

```rust
let resources = client.list_resources().await?;

for resource in resources {
    println!("Resource: {} ({})", resource.name, resource.uri);
    if let Some(desc) = resource.description {
        println!("  Description: {}", desc);
    }
    if let Some(mime) = resource.mime_type {
        println!("  MIME type: {}", mime);
    }
}
```

#### ResourceInfo

```rust
pub struct ResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}
```

### Reading Resources

Read resource content:

```rust
let content = client.read_resource("file:///path/to/file.txt").await?;

for item in content.contents {
    match item {
        ResourceContent::Text { uri, mime_type, text } => {
            println!("Text resource: {}", uri);
            println!("Content: {}", text);
        }
        ResourceContent::Blob { uri, mime_type, blob } => {
            println!("Binary resource: {}", uri);
            println!("Size: {} bytes", blob.len());
        }
    }
}
```

#### ReadResourceResult

```rust
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContent>,
}

pub enum ResourceContent {
    Text {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
    Blob {
        uri: String,
        mime_type: Option<String>,
        blob: Vec<u8>,
    },
}
```

### Resource Subscriptions

Subscribe to resource updates:

```rust
// Subscribe to updates
client.subscribe_resource("config://app").await?;

// Listen for updates
while let Some(update) = client.receive_resource_update().await {
    println!("Resource updated: {}", update.uri);
    // Re-read the resource
    let content = client.read_resource(&update.uri).await?;
}

// Unsubscribe
client.unsubscribe_resource("config://app").await?;
```

## Prompt Operations

### Listing Prompts

Get all available prompts:

```rust
let prompts = client.list_prompts().await?;

for prompt in prompts {
    println!("Prompt: {}", prompt.name);
    if let Some(desc) = prompt.description {
        println!("  Description: {}", desc);
    }
    if let Some(args) = prompt.arguments {
        println!("  Arguments:");
        for arg in args {
            println!("    - {}: {}", arg.name, arg.description.unwrap_or_default());
        }
    }
}
```

#### PromptInfo

```rust
pub struct PromptInfo {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}

pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}
```

### Getting Prompts

Retrieve a prompt with arguments:

```rust
let mut args = HashMap::new();
args.insert("language".to_string(), serde_json::json!("Rust"));
args.insert("topic".to_string(), serde_json::json!("async programming"));

let prompt_result = client.get_prompt("code_review", Some(args)).await?;

for message in prompt_result.messages {
    println!("[{}] {}", message.role, message.content);
}
```

#### GetPromptResult

```rust
pub struct GetPromptResult {
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}

pub struct PromptMessage {
    pub role: MessageRole,
    pub content: String,
}

pub enum MessageRole {
    User,
    Assistant,
    System,
}
```

## Error Handling

### Client Errors

Handle client-specific errors:

```rust
use turbomcp_client::ClientError;

match client.call_tool("my_tool", None).await {
    Ok(result) => {
        println!("Success: {:?}", result);
    }
    Err(ClientError::TransportError(e)) => {
        eprintln!("Transport error: {}", e);
    }
    Err(ClientError::TimeoutError) => {
        eprintln!("Request timed out");
    }
    Err(ClientError::ServerError { code, message }) => {
        eprintln!("Server error {}: {}", code, message);
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

#### ClientError Types

```rust
pub enum ClientError {
    TransportError(String),
    TimeoutError,
    ConnectionClosed,
    ServerError { code: i64, message: String },
    ParseError(String),
    InvalidResponse(String),
}
```

### Retry Logic

Implement custom retry logic:

```rust
use tokio::time::{sleep, Duration};

async fn call_with_retry<T>(
    operation: impl Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>>>>,
    max_retries: u32,
) -> Result<T> {
    let mut retries = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if retries < max_retries => {
                retries += 1;
                let backoff = Duration::from_millis(100 * 2_u64.pow(retries));
                sleep(backoff).await;
            }
            Err(e) => return Err(e),
        }
    }
}

// Usage
let result = call_with_retry(
    || Box::pin(client.call_tool("flaky_tool", None)),
    3
).await?;
```

## Advanced Features

### Request Timeout

Configure per-request timeouts:

```rust
use tokio::time::{timeout, Duration};

let result = timeout(
    Duration::from_secs(10),
    client.call_tool("slow_tool", None)
).await??;
```

### Parallel Requests

Execute multiple requests concurrently:

```rust
use futures::future::try_join_all;

let tools = vec!["tool1", "tool2", "tool3"];
let futures: Vec<_> = tools.into_iter()
    .map(|name| client.call_tool(name, None))
    .collect();

let results = try_join_all(futures).await?;
```

### Custom Headers

Add custom headers to HTTP/WebSocket transports:

```rust
let mut transport = HttpTransport::new("http://localhost:8080");
transport.add_header("Authorization", "Bearer token123");
transport.add_header("X-Custom-Header", "value");

let client = Client::new(transport);
```

### Connection Pooling

Reuse client connections efficiently:

```rust
use std::sync::Arc;

// Wrap client in Arc for sharing
let client = Arc::new(Client::new(transport));

// Clone Arc for concurrent access
let client1 = client.clone();
let client2 = client.clone();

// Use in parallel
let (result1, result2) = tokio::join!(
    client1.call_tool("tool1", None),
    client2.call_tool("tool2", None)
);
```

## Transport Types

### StdioTransport

For local process communication:

```rust
let transport = StdioTransport::new();
let client = Client::new(transport);
```

#### Configuration

```rust
let transport = StdioTransport::builder()
    .buffer_size(8192)
    .timeout(Duration::from_secs(30))
    .build();
```

### HttpTransport

For HTTP/SSE communication:

```rust
let transport = HttpTransport::new("http://localhost:8080");
let client = Client::new(transport);
```

#### Configuration

```rust
let transport = HttpTransport::builder("http://localhost:8080")
    .timeout(Duration::from_secs(30))
    .header("Authorization", "Bearer token")
    .compression(true)
    .build()?;
```

### WebSocketTransport

For WebSocket communication:

```rust
let transport = WebSocketTransport::connect("ws://localhost:8081").await?;
let client = Client::new(transport);
```

#### Configuration

```rust
let transport = WebSocketTransport::builder("ws://localhost:8081")
    .timeout(Duration::from_secs(30))
    .ping_interval(Duration::from_secs(30))
    .reconnect_on_disconnect(true)
    .build()
    .await?;
```

### TcpTransport

For TCP socket communication:

```rust
let transport = TcpTransport::connect("localhost:9000").await?;
let client = Client::new(transport);
```

## Testing

### Mock Client

Create mock clients for testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_with_mock() {
        let transport = MockTransport::new();
        transport.expect_call("tool1", json!({"result": "success"}));

        let client = Client::new(transport);
        let result = client.call_tool("tool1", None).await.unwrap();

        assert_eq!(result.content.as_text().unwrap(), "success");
    }
}
```

### Integration Testing

```rust
#[tokio::test]
async fn test_full_workflow() {
    // Start test server
    let server = tokio::spawn(async {
        TestServer.run_stdio().await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create client
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Test operations
    client.initialize().await.unwrap();
    let tools = client.list_tools().await.unwrap();
    assert!(!tools.is_empty());

    // Cleanup
    server.abort();
}
```

## Best Practices

### 1. Always Initialize

```rust
// Good
let client = Client::new(transport);
client.initialize().await?;
// Now safe to use

// Avoid
let client = Client::new(transport);
client.call_tool("tool", None).await?; // May fail
```

### 2. Handle Connection Errors

```rust
// Good
match client.call_tool("tool", None).await {
    Ok(result) => process_result(result),
    Err(ClientError::ConnectionClosed) => {
        // Reconnect logic
        client.reconnect().await?;
    }
    Err(e) => return Err(e),
}

// Avoid
let result = client.call_tool("tool", None).await.unwrap();
```

### 3. Use Timeouts

```rust
// Good
use tokio::time::{timeout, Duration};

let result = timeout(
    Duration::from_secs(30),
    client.call_tool("tool", None)
).await??;

// Avoid
let result = client.call_tool("tool", None).await?; // May hang forever
```

### 4. Validate Server Capabilities

```rust
// Good
let init = client.initialize().await?;
if init.capabilities.tools.is_none() {
    return Err("Server does not support tools".into());
}

// Avoid
client.call_tool("tool", None).await?; // May not be supported
```

### 5. Clean Up Resources

```rust
// Good
{
    let client = Client::new(transport);
    client.initialize().await?;
    // Use client
} // Client dropped, connection closed

// Or explicitly
client.disconnect().await?;
```

## Examples

### Complete Client Application

```rust
use turbomcp_client::prelude::*;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    // Create client
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Initialize
    let init = client.initialize().await?;
    println!("Connected to {} v{}",
        init.server_info.name,
        init.server_info.version
    );

    // List and call tools
    let tools = client.list_tools().await?;
    for tool in tools {
        println!("Tool: {}", tool.name);

        // Call tool with sample args
        let mut args = HashMap::new();
        args.insert("test".to_string(), json!("value"));

        match client.call_tool(&tool.name, Some(args)).await {
            Ok(result) => println!("  Result: {:?}", result),
            Err(e) => eprintln!("  Error: {}", e),
        }
    }

    // Clean up
    client.disconnect().await?;
    Ok(())
}
```

### Concurrent Tool Calls

```rust
use futures::future::try_join_all;

async fn call_multiple_tools(client: &Client) -> Result<Vec<ToolCallResult>> {
    let tool_names = vec!["tool1", "tool2", "tool3"];

    let futures: Vec<_> = tool_names.into_iter()
        .map(|name| client.call_tool(name, None))
        .collect();

    try_join_all(futures).await
}
```

### Resource Monitoring

```rust
async fn monitor_resource(client: &Client, uri: &str) -> Result<()> {
    client.subscribe_resource(uri).await?;

    loop {
        tokio::select! {
            Some(update) = client.receive_resource_update() => {
                println!("Resource {} updated", update.uri);
                let content = client.read_resource(&update.uri).await?;
                process_content(content);
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    client.unsubscribe_resource(uri).await?;
    Ok(())
}
```

## Troubleshooting

### "Connection refused"

Server may not be running or address is incorrect:

```rust
// Check server is running
// Verify address and port
let transport = HttpTransport::new("http://localhost:8080"); // Correct port?
```

### "Request timeout"

Increase timeout or check server performance:

```rust
let transport = HttpTransport::builder("http://localhost:8080")
    .timeout(Duration::from_secs(60)) // Increase timeout
    .build()?;
```

### "Invalid response format"

Server may not be MCP-compliant:

```rust
// Enable debug logging
tracing_subscriber::fmt()
    .with_env_filter("debug")
    .init();

// Check server compatibility
let init = client.initialize().await?;
println!("Protocol version: {}", init.protocol_version);
```

## Next Steps

- **[Server API](server.md)** - Build MCP servers
- **[Transports Guide](../guide/transports.md)** - Transport configuration
- **[Examples](../examples/basic.md)** - Real-world client examples

## See Also

- [MCP Specification](https://modelcontextprotocol.io/specification)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp-client)
- [Source Code](https://github.com/yourusername/turbomcp)
