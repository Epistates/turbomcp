# Client API Reference

Complete API reference for building MCP clients with TurboMCP.

## Overview

The TurboMCP client API provides a complete implementation for connecting to MCP servers, calling tools, reading resources, and managing prompts. The client handles connection management, request correlation, server-initiated requests, and capability negotiation.

The examples on this page use `turbomcp-client` directly. Through the `turbomcp` facade the same client is available with the `client-integration` feature (STDIO) or `full-client` (all transports), as `turbomcp::turbomcp_client` and `turbomcp::prelude::Client`.

## Core Types

### Client

The main client type for interacting with MCP servers. `Client<T>` is generic over its transport and cheap to clone: clones share one connection.

```rust
use turbomcp_client::prelude::*;

let transport = StdioTransport::new();
let client = Client::new(transport);
```

#### Creating a Client

`turbomcp-client` re-exports each transport its features enable. The `connect_*` helpers build the transport, connect, and run the `initialize` handshake in one call:

```rust
use std::net::SocketAddr;
use turbomcp_client::{
    Client, StdioTransport, TcpTransport, WebSocketBidirectionalConfig,
    WebSocketBidirectionalTransport,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // STDIO transport (for a client spawned by, or spawning, an MCP process)
    let client = Client::new(StdioTransport::new());
    client.initialize().await?;

    // Streamable HTTP (feature `http`): connects to http://localhost:8080/mcp
    let http = Client::connect_http("http://localhost:8080").await?;

    // WebSocket (feature `websocket`)
    let config = WebSocketBidirectionalConfig::client("ws://localhost:8081".to_string());
    let ws = Client::new(WebSocketBidirectionalTransport::new(config).await?);
    ws.initialize().await?;

    // TCP (feature `tcp`)
    let tcp = Client::connect_tcp("127.0.0.1:9000").await?;
    // ...or build the transport yourself
    let server: SocketAddr = "127.0.0.1:9000".parse()?;
    let bind: SocketAddr = "0.0.0.0:0".parse()?;
    let tcp2 = Client::new(TcpTransport::new_client(bind, server));
    tcp2.initialize().await?;

    // Unix socket (feature `unix`)
    let unix = Client::connect_unix("/tmp/mcp.sock").await?;
    Ok(())
}
```

## Connection Management

### Initialization

Initialize the connection and perform capability negotiation:

```rust
use turbomcp_client::{Client, Transport};

async fn connect(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    let init_result = client.initialize().await?;

    println!("Server: {} v{} (protocol {})",
        init_result.server_info.name,
        init_result.server_info.version,
        init_result.protocol_version,
    );

    // Check server capabilities
    let caps = &init_result.server_capabilities;
    if caps.tools.is_some() {
        println!("Server supports tools");
    }
    if caps.resources.is_some() {
        println!("Server supports resources");
    }
    if caps.prompts.is_some() {
        println!("Server supports prompts");
    }
    if let Some(instructions) = &init_result.instructions {
        println!("Instructions: {instructions}");
    }
    Ok(())
}
```

#### InitializeResult

Abridged; see [docs.rs](https://docs.rs/turbomcp-client) for the full definitions:

```rust,ignore
pub struct InitializeResult {
    pub server_info: Implementation,          // name, version, title, ...
    pub server_capabilities: ServerCapabilities,
    pub protocol_version: String,             // the negotiated version
    pub instructions: Option<String>,
}

pub struct ServerCapabilities {
    pub tools: Option<ToolsCapabilities>,
    pub resources: Option<ResourcesCapabilities>,
    pub prompts: Option<PromptsCapabilities>,
    pub logging: Option<LoggingCapabilities>,
    pub completions: Option<CompletionCapabilities>,
    // tasks, extensions, experimental
}
```

### Connection State

```rust
use turbomcp_client::{Client, Transport};

async fn state(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // Has the initialize handshake completed?
    if client.is_initialized() {
        println!("Negotiated {:?}", client.negotiated_protocol_version());
        println!("Capabilities: {:?}", client.server_capabilities());
    }

    // Check the server is responsive
    client.ping().await?;

    // Close the connection. For Streamable HTTP this also ends the session.
    client.shutdown().await?;
    Ok(())
}
```

## Tool Operations

### Listing Tools

Get all available tools from the server. `list_tools` follows pagination cursors; `list_tools_paginated(cursor)` returns one page.

```rust
use turbomcp_client::{Client, Transport};

async fn show_tools(client: &Client<impl Transport + 'static>) -> Result<(), Box<dyn std::error::Error>> {
    let tools = client.list_tools().await?;

    for tool in tools {
        println!("Tool: {}", tool.name);
        if let Some(desc) = &tool.description {
            println!("  Description: {}", desc);
        }
        println!("  Schema: {}", serde_json::to_string_pretty(&tool.input_schema)?);
    }
    Ok(())
}
```

#### Tool

Abridged:

```rust,ignore
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: ToolInputSchema,
    pub title: Option<String>,
    pub icons: Option<Vec<Icon>>,
    pub annotations: Option<ToolAnnotations>,
    pub output_schema: Option<ToolOutputSchema>,
    // execution, _meta
}
```

### Calling Tools

Execute a tool on the server. Arguments are a map from name to JSON value; the last parameter is an optional task augmentation (`None` for an ordinary call):

```rust
use std::collections::HashMap;
use turbomcp_client::{Client, ContentBlock, Transport};

async fn weather(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // Build arguments
    let mut args = HashMap::new();
    args.insert("city".to_string(), serde_json::json!("San Francisco"));
    args.insert("units".to_string(), serde_json::json!("metric"));

    // Call tool
    let result = client.call_tool("get_weather", Some(args), None).await?;

    // A tool that ran and failed returns Ok with is_error set
    if result.is_error == Some(true) {
        eprintln!("Tool reported an error");
    }

    // Parse result
    for block in &result.content {
        match block {
            ContentBlock::Text(text) => println!("Result: {}", text.text),
            ContentBlock::Image(image) => {
                println!("Got image: {} ({} base64 bytes)", image.mime_type, image.data.len())
            }
            ContentBlock::Resource(embedded) => println!("Got resource: {:?}", embedded.resource),
            other => println!("Other content: {:?}", other),
        }
    }

    // Structured output, when the tool declares an output schema
    if let Some(structured) = &result.structured_content {
        println!("Structured: {structured}");
    }
    Ok(())
}
```

#### CallToolResult

Abridged:

```rust,ignore
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,           // Text, Image, Audio, ResourceLink, Resource
    pub is_error: Option<bool>,
    pub structured_content: Option<serde_json::Value>,
    // _meta
}
```

### Tool Call Options

A per-request timeout comes from `with_timeout`, which returns a handle to the same client. When a request times out, the server is sent `notifications/cancelled` for it. Retries are configured on the transport (see [Retry Logic](#retry-logic)).

```rust
use std::time::Duration;
use turbomcp_client::{Client, Transport};

async fn long_operation(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    let result = client
        .with_timeout(Duration::from_secs(300))
        .call_tool("long_operation", None, None)
        .await?;
    Ok(())
}
```

## Resource Operations

### Listing Resources

Get all available resources:

```rust
use turbomcp_client::{Client, Transport};

async fn show_resources(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
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

    // Parameterised resources are listed separately
    let templates = client.list_resource_templates().await?;
    Ok(())
}
```

### Reading Resources

Read resource content:

```rust
use turbomcp_client::{Client, ResourceContents, Transport};

async fn read(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    let content = client.read_resource("file:///path/to/file.txt").await?;

    for item in content.contents {
        match item {
            ResourceContents::Text(text) => {
                println!("Text resource: {}", text.uri);
                println!("Content: {}", text.text);
            }
            ResourceContents::Blob(blob) => {
                println!("Binary resource: {}", blob.uri);
                println!("Size: {} base64 bytes", blob.blob.len());
            }
        }
    }
    Ok(())
}
```

#### ReadResourceResult

Abridged:

```rust,ignore
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
}

pub enum ResourceContents {
    Text(TextResourceContents),   // uri, mime_type, text
    Blob(BlobResourceContents),   // uri, mime_type, blob (base64)
}
```

### Resource Subscriptions

Subscribe to resource updates. Updates arrive as `notifications/resources/updated`, delivered to a `ResourceUpdateHandler`:

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use turbomcp_client::handlers::{HandlerResult, ResourceUpdateHandler, ResourceUpdatedNotification};
use turbomcp_client::{Client, Transport};

#[derive(Debug)]
struct PrintUpdates;

impl ResourceUpdateHandler for PrintUpdates {
    fn handle_resource_update(
        &self,
        notification: ResourceUpdatedNotification,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<()>> + Send + '_>> {
        Box::pin(async move {
            println!("Resource updated: {}", notification.uri);
            Ok(())
        })
    }
}

async fn watch(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    client.set_resource_update_handler(Arc::new(PrintUpdates));

    // Subscribe to updates (the server must advertise resources.subscribe)
    client.subscribe("config://app").await?;

    // ... later
    client.unsubscribe("config://app").await?;
    Ok(())
}
```

## Prompt Operations

### Listing Prompts

Get all available prompts:

```rust
use turbomcp_client::{Client, Transport};

async fn show_prompts(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
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
    Ok(())
}
```

#### Prompt

Abridged:

```rust,ignore
pub struct Prompt {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
    // icons, _meta
}

pub struct PromptArgument {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: Option<bool>,
}
```

### Getting Prompts

Retrieve a prompt with arguments:

```rust
use turbomcp_client::{Client, Transport};
use turbomcp_protocol::types::PromptInput;

async fn review(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // PromptInput is HashMap<String, serde_json::Value>
    let mut args = PromptInput::new();
    args.insert("language".to_string(), serde_json::json!("Rust"));
    args.insert("topic".to_string(), serde_json::json!("async programming"));

    let prompt_result = client.get_prompt("code_review", Some(args)).await?;

    for message in prompt_result.messages {
        println!("[{:?}] {:?}", message.role, message.content);
    }
    Ok(())
}
```

#### GetPromptResult

Abridged:

```rust,ignore
pub struct GetPromptResult {
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,  // role: Role (User | Assistant), content: ContentBlock
}
```

On the server side, prompt handlers build their result with the ergonomic constructors on `turbomcp::PromptResult` and `turbomcp::Message`:

```rust
use turbomcp::prelude::*;

// Single message
let result = PromptResult::user("Hello!");

// Multi-message with builder
let result = PromptResult::user("Question")
    .add_assistant("Answer")
    .with_description("A conversation");

// Direct message construction
let msg = Message::user("Hello!");
let msg = Message::assistant("Hi there!");
```

## Error Handling

### Client Errors

`turbomcp_client::Error` is `McpError`, a struct with a classification (`kind: ErrorKind`) and a message, not an enum of variants. Match on `err.kind`:

```rust
use turbomcp_client::{Client, Transport};
use turbomcp_protocol::ErrorKind;

async fn call(client: &Client<impl Transport + 'static>) {
    match client.call_tool("my_tool", None, None).await {
        Ok(result) => {
            println!("Success: {:?}", result);
        }
        Err(e) if e.kind == ErrorKind::Transport => {
            eprintln!("Transport error: {}", e);
        }
        Err(e) if e.kind == ErrorKind::Timeout => {
            eprintln!("Request timed out");
        }
        Err(e) => {
            eprintln!("Error {} ({:?}): {}", e.jsonrpc_code(), e.kind, e.message);
        }
    }
}
```

#### ErrorKind

Abridged:

```rust,ignore
pub enum ErrorKind {
    // MCP-specific
    ToolNotFound, ToolExecutionFailed, PromptNotFound, ResourceNotFound,
    ResourceAccessDenied, CapabilityNotSupported, ProtocolVersionMismatch,
    UrlElicitationRequired, UserRejected,
    // JSON-RPC
    ParseError, InvalidRequest, MethodNotFound, InvalidParams, Internal,
    // General
    Authentication, PermissionDenied, Transport, Timeout, Unavailable,
    RateLimited, ServerOverloaded, Configuration, ExternalService,
    Cancelled, Security, Serialization,
}
```

`err.is_retryable()` reports whether retrying may succeed.

### Retry Logic

`ClientBuilder::build_resilient` wraps the transport with retry, a circuit breaker, and health checking:

```rust
use std::time::Duration;
use turbomcp_client::{ClientBuilder, StdioTransport};
use turbomcp_transport::resilience::RetryConfig;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = ClientBuilder::new()
        .with_retry_config(RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            ..Default::default()
        })
        .build_resilient(StdioTransport::new())
        .await?;
    client.initialize().await?;
    Ok(())
}
```

Or retry an individual call yourself:

```rust
use std::future::Future;
use tokio::time::{sleep, Duration};
use turbomcp_client::{Client, Transport};

async fn call_with_retry<T, F, Fut>(operation: F, max_retries: u32) -> turbomcp_client::Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = turbomcp_client::Result<T>>,
{
    let mut retries = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if retries < max_retries && e.is_retryable() => {
                retries += 1;
                let backoff = Duration::from_millis(100 * 2_u64.pow(retries));
                sleep(backoff).await;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn flaky(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    let result = call_with_retry(|| client.call_tool("flaky_tool", None, None), 3).await?;
    Ok(())
}
```

## Advanced Features

### Request Timeout

Configure per-request timeouts with `with_timeout` (see [Tool Call Options](#tool-call-options)), or the default for every request when building the client:

```rust
use turbomcp_client::{ClientBuilder, StdioTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = ClientBuilder::new()
        .with_timeout(10_000) // milliseconds
        .build(StdioTransport::new())
        .await?;
    client.initialize().await?;
    Ok(())
}
```

### Parallel Requests

Execute multiple requests concurrently:

```rust
use futures::future::try_join_all;
use turbomcp_client::{Client, Transport};

async fn parallel(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    let tools = vec!["tool1", "tool2", "tool3"];
    let futures: Vec<_> = tools
        .into_iter()
        .map(|name| client.call_tool(name, None, None))
        .collect();

    let results = try_join_all(futures).await?;
    Ok(())
}
```

### Custom Headers

Add custom headers, or a bearer token, to the Streamable HTTP transport:

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::connect_http_with("http://localhost:8080", |config| {
        config.auth_token = Some("token123".to_string());
        config.headers.insert("X-Custom-Header".to_string(), "value".to_string());
    })
    .await?;
    Ok(())
}
```

To obtain the token when the server challenges, set `config.auth_provider` to a `turbomcp_http::AuthProvider`; see the `turbomcp-client` README.

### Sharing a Client

`Client` is already reference-counted, so clone it rather than wrapping it in an `Arc`:

```rust
use turbomcp_client::{Client, Transport};

async fn share(client: &Client<impl Transport + 'static>) {
    let client1 = client.clone();
    let client2 = client.clone();

    // Use in parallel
    let (result1, result2) = tokio::join!(
        client1.call_tool("tool1", None, None),
        client2.call_tool("tool2", None, None)
    );
}
```

## Transport Types

### StdioTransport

For communication over the process's stdin/stdout:

```rust
use turbomcp_client::{Client, StdioTransport};

let transport = StdioTransport::new();
let client = Client::new(transport);
```

#### Configuration

`StdioTransport::with_config` takes a `TransportConfig`; `from_child` and `from_raw` attach to a spawned process or arbitrary reader/writer.

```rust
use std::time::Duration;
use turbomcp_client::StdioTransport;
use turbomcp_transport::TransportConfig;

let transport = StdioTransport::with_config(TransportConfig {
    connect_timeout: Duration::from_secs(30),
    ..Default::default()
});
```

### StreamableHttpClientTransport

For Streamable HTTP (POST plus SSE):

```rust
use turbomcp_client::{Client, StreamableHttpClientConfig, StreamableHttpClientTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = StreamableHttpClientTransport::new(StreamableHttpClientConfig {
        base_url: "http://localhost:8080".to_string(),
        ..Default::default()
    })?;
    let client = Client::new(transport);
    client.initialize().await?;
    Ok(())
}
```

#### Configuration

```rust
use std::time::Duration;
use turbomcp_client::StreamableHttpClientConfig;

let config = StreamableHttpClientConfig {
    base_url: "http://localhost:8080".to_string(),
    endpoint_path: "/mcp".to_string(),
    timeout: Duration::from_secs(30),
    auth_token: Some("token".to_string()),
    ..Default::default()
};
```

### WebSocketBidirectionalTransport

For WebSocket communication:

```rust
use std::time::Duration;
use turbomcp_client::{Client, WebSocketBidirectionalConfig, WebSocketBidirectionalTransport};
use turbomcp_transport::websocket_bidirectional::ReconnectConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WebSocketBidirectionalConfig::client("ws://localhost:8081".to_string())
        .with_keep_alive_interval(Duration::from_secs(30))
        .with_reconnect_config(ReconnectConfig::new().with_enabled(true));

    let transport = WebSocketBidirectionalTransport::new(config).await?;
    let client = Client::new(transport);
    client.initialize().await?;
    Ok(())
}
```

### TcpTransport

For TCP socket communication:

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::connect_tcp("127.0.0.1:9000").await?;
    Ok(())
}
```

## Testing

### Testing Against a Server In-Process

To test server logic without a transport, use `turbomcp::testing::McpTestClient`, which dispatches straight into a handler:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct TestServer;

#[server]
impl TestServer {
    /// Always succeeds
    #[tool]
    async fn tool1(&self) -> String {
        "success".to_string()
    }
}

#[tokio::test]
async fn test_with_test_client() {
    let client = McpTestClient::new(TestServer);
    let result = client.call_tool_empty("tool1").await.unwrap();
    assert_eq!(result.first_text(), Some("success"));
}
```

### Integration Testing

For an end-to-end test, run the server on a real transport and connect to it:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct TestServer;

#[server]
impl TestServer {
    /// Always succeeds
    #[tool]
    async fn tool1(&self) -> String {
        "success".to_string()
    }
}

#[tokio::test]
async fn test_full_workflow() {
    // Start test server (features `tcp` and `full-client`)
    let server = tokio::spawn(async { TestServer.run_tcp("127.0.0.1:9123").await });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect and initialize
    let client = Client::connect_tcp("127.0.0.1:9123").await.unwrap();

    // Test operations
    let tools = client.list_tools().await.unwrap();
    assert!(!tools.is_empty());

    // Cleanup
    client.shutdown().await.unwrap();
    server.abort();
}
```

## Best Practices

### 1. Always Initialize

`Client::new` and `ClientBuilder::build` do not connect. Initialize before anything else; other requests fail with `Client not initialized` until you do. The `connect_*` helpers initialize for you.

```rust
use turbomcp_client::{Client, StdioTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::new(StdioTransport::new());
    client.initialize().await?;
    // Now safe to use
    client.call_tool("tool", None, None).await?;
    Ok(())
}
```

### 2. Handle Connection Errors

```rust
use turbomcp_client::{Client, Transport};
use turbomcp_protocol::ErrorKind;

async fn call(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    match client.call_tool("tool", None, None).await {
        Ok(result) => println!("{result:?}"),
        Err(e) if e.kind == ErrorKind::Transport => {
            // The connection is gone: build a new client (or use build_resilient)
            eprintln!("connection lost: {e}");
        }
        Err(e) => return Err(e),
    }
    Ok(())
}
```

### 3. Use Timeouts

Every request already has the transport's timeout (30 s by default). Raise it for a slow call with `with_timeout` rather than wrapping the future in `tokio::time::timeout`: the client then also tells the server to cancel the request.

```rust
use std::time::Duration;
use turbomcp_client::{Client, Transport};

async fn slow(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    let result = client
        .with_timeout(Duration::from_secs(120))
        .call_tool("tool", None, None)
        .await?;
    Ok(())
}
```

### 4. Validate Server Capabilities

```rust
use turbomcp_client::{Client, Transport};

async fn tools_supported(client: &Client<impl Transport + 'static>) -> Result<(), Box<dyn std::error::Error>> {
    let init = client.initialize().await?;
    if init.server_capabilities.tools.is_none() {
        return Err("Server does not support tools".into());
    }
    Ok(())
}
```

### 5. Clean Up Resources

Call `shutdown` when done. Dropping the last clone without it only logs a warning, and for Streamable HTTP leaves the server's session open until it expires.

```rust
use turbomcp_client::{Client, Transport};

async fn done(client: Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    client.shutdown().await
}
```

## Examples

### Complete Client Application

```rust
use std::collections::HashMap;
use turbomcp_client::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging (to stderr: stdout carries the protocol)
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    // Create client
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Initialize
    let init = client.initialize().await?;
    eprintln!("Connected to {} v{}",
        init.server_info.name,
        init.server_info.version
    );

    // List and call tools
    let tools = client.list_tools().await?;
    for tool in tools {
        eprintln!("Tool: {}", tool.name);

        // Call tool with sample args
        let mut args = HashMap::new();
        args.insert("test".to_string(), serde_json::json!("value"));

        match client.call_tool(&tool.name, Some(args), None).await {
            Ok(result) => eprintln!("  Result: {:?}", result),
            Err(e) => eprintln!("  Error: {}", e),
        }
    }

    // Clean up
    client.shutdown().await?;
    Ok(())
}
```

### Concurrent Tool Calls

```rust
use futures::future::try_join_all;
use turbomcp_client::{CallToolResult, Client, Result, Transport};

async fn call_multiple_tools(client: &Client<impl Transport + 'static>) -> Result<Vec<CallToolResult>> {
    let tool_names = vec!["tool1", "tool2", "tool3"];

    let futures: Vec<_> = tool_names
        .into_iter()
        .map(|name| client.call_tool(name, None, None))
        .collect();

    try_join_all(futures).await
}
```

### Resource Monitoring

Pair a `ResourceUpdateHandler` (see [Resource Subscriptions](#resource-subscriptions)) that forwards update URIs over a channel with a loop that re-reads them:

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use turbomcp_client::handlers::{HandlerResult, ResourceUpdateHandler, ResourceUpdatedNotification};
use turbomcp_client::{Client, Result, Transport};

#[derive(Debug)]
struct Forward(mpsc::UnboundedSender<String>);

impl ResourceUpdateHandler for Forward {
    fn handle_resource_update(
        &self,
        notification: ResourceUpdatedNotification,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<()>> + Send + '_>> {
        let _ = self.0.send(notification.uri.to_string());
        Box::pin(async { Ok(()) })
    }
}

async fn monitor_resource(client: &Client<impl Transport + 'static>, uri: &str) -> Result<()> {
    let (tx, mut updates) = mpsc::unbounded_channel();
    client.set_resource_update_handler(Arc::new(Forward(tx)));
    client.subscribe(uri).await?;

    loop {
        tokio::select! {
            Some(updated) = updates.recv() => {
                println!("Resource {} updated", updated);
                let content = client.read_resource(&updated).await?;
                println!("{} content item(s)", content.contents.len());
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    client.unsubscribe(uri).await?;
    Ok(())
}
```

## Troubleshooting

### "Connection refused"

The server may not be running, or the address is wrong. For Streamable HTTP, `connect_http` takes the base URL and appends the endpoint path (`/mcp` by default):

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // Requests go to http://localhost:8080/mcp. Correct port?
    let client = Client::connect_http("http://localhost:8080").await?;
    Ok(())
}
```

### "Request timeout"

Increase the timeout or check server performance:

```rust
use std::time::Duration;
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::connect_http_with("http://localhost:8080", |config| {
        config.timeout = Duration::from_secs(60); // Increase timeout
    })
    .await?;
    Ok(())
}
```

### "Invalid response format"

The server may not be MCP-compliant:

```rust
use turbomcp_client::{Client, Transport};

async fn diagnose(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // Enable debug logging
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .with_writer(std::io::stderr)
        .init();

    // Check server compatibility
    let init = client.initialize().await?;
    println!("Protocol version: {}", init.protocol_version);
    Ok(())
}
```

## Next Steps

- **[Server API](server.md)** - Build MCP servers
- **[Transports Guide](../guide/transports.md)** - Transport configuration
- **[Examples](../examples/basic.md)** - Real-world client examples

## See Also

- [MCP Specification](https://modelcontextprotocol.io/specification)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp-client)
- [Source Code](https://github.com/Epistates/turbomcp)
