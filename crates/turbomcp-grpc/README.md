# turbomcp-grpc

High-performance gRPC transport for the Model Context Protocol (MCP).

## Features

- **Server**: Full gRPC server implementation with streaming notifications
- **Client**: gRPC client with automatic initialization and reconnection
- **Tower Integration**: Composable middleware via Tower layers
- **TLS**: Optional TLS 1.3 support via rustls
- **Streaming**: Server-streaming for real-time notifications

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
turbomcp-grpc = "3.0.0-alpha.1"
```

## Quick Start

### Server

```rust
use turbomcp_grpc::server::McpGrpcServer;
use turbomcp_core::types::tools::Tool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpGrpcServer::builder()
        .server_info("my-server", "1.0.0")
        .add_tool(Tool {
            name: "hello".to_string(),
            description: Some("Says hello".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                }
            }),
            annotations: None,
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
use turbomcp_grpc::client::McpGrpcClient;

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

The gRPC service is defined in `src/proto/mcp.proto` and includes:

- `Initialize` - Session initialization
- `Ping` - Health check
- `ListTools` / `CallTool` - Tool operations
- `ListResources` / `ReadResource` - Resource operations
- `ListPrompts` / `GetPrompt` - Prompt operations
- `Subscribe` - Streaming notifications
- `Complete` - Autocomplete suggestions
- `SetLoggingLevel` - Logging configuration

## Tower Integration

Use with Tower layers for composable middleware:

```rust
use turbomcp_grpc::layer::McpGrpcLayer;
use tower::ServiceBuilder;

let layer = McpGrpcLayer::new()
    .timeout(Duration::from_secs(30))
    .logging(true)
    .timing(true);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(inner_service);
```

## Features

- `server` (default) - Enable server implementation
- `client` (default) - Enable client implementation
- `health` - Enable gRPC health checking service
- `reflection` - Enable gRPC reflection for debugging
- `tls` - Enable TLS support

## License

MIT
