# turbomcp-grpc

High-performance gRPC transport for the Model Context Protocol (MCP).

## Features

- **Server**: Full gRPC server implementation with streaming notifications
- **Client**: gRPC client wrapper with explicit session initialization
- **Tower Integration**: Composable middleware via Tower layers
- **TLS**: Configured through tonic's transport builders
- **Streaming**: Server-streaming for real-time notifications

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
turbomcp-grpc = "3.1.6"
```

## Quick Start

### Server

```rust
use turbomcp_grpc::McpGrpcServer;
use turbomcp_types::{Tool, ToolInputSchema};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpGrpcServer::builder()
        .server_info("my-server", "1.0.0")
        .add_tool(Tool {
            name: "hello".to_string(),
            description: Some("Says hello".to_string()),
            input_schema: ToolInputSchema::from_value(serde_json::json!({
                "type": "object",
                "properties": { "name": {"type": "string"} }
            })),
            title: None,
            icons: None,
            annotations: None,
            output_schema: None,
            meta: None,
        })
        .build();

    tonic::transport::Server::builder()
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;

    Ok(())
}
```

### Client

```rust
use turbomcp_grpc::McpGrpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = McpGrpcClient::connect("http://[::1]:50051").await?;

    // Initialize the session
    let init_result = client.initialize().await?;
    println!("Connected to: {:?}", init_result.server_info);

    // List available tools
    let tools = client.list_tools().await?;
    println!("Available tools: {:?}", tools);

    // Call a tool
    let result = client.call_tool("hello", Some(serde_json::json!({"name": "World"}))).await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

## Protocol Definition

The gRPC service is defined in `src/proto/mcp.proto` (package `turbomcp.mcp.v1`) and includes:

- `Initialize` - Session initialization
- `Ping` - Health check
- `ListTools` / `CallTool` - Tool operations
- `ListResources` / `ListResourceTemplates` / `ReadResource` - Resource operations
- `ListPrompts` / `GetPrompt` - Prompt operations
- `Subscribe` - Server-streaming notifications
- `Complete` - Autocomplete suggestions
- `SetLoggingLevel` - Logging configuration
- `ListRoots` / `CreateSamplingMessage` / `Elicit` - Client-capability bridges

## Tower Integration

Use with Tower layers for composable middleware:

```rust
use turbomcp_grpc::McpGrpcLayer;
use tower::ServiceBuilder;

let layer = McpGrpcLayer::new()
    .logging(true)
    .timing(true);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(inner_service);
```

## Features

- `server` (default) - Enable server implementation
- `client` (default) - Enable client implementation
- `health` - Compatibility feature; use MCP `Ping` or add `tonic-health` directly
- `reflection` - Reserved compatibility feature
- `tls` - Reserved compatibility feature; TLS is configured through tonic

## License

MIT
