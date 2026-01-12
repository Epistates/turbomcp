# gRPC Transport API Reference

The `turbomcp-grpc` crate provides high-performance gRPC transport for MCP in TurboMCP v3.

## Overview

gRPC transport offers:

- **Full MCP Protocol** - All operations via efficient gRPC calls
- **Streaming** - Server-streaming for real-time notifications
- **Tower Integration** - Composable middleware via Tower layers
- **TLS** - Optional TLS 1.3 support via rustls
- **Health Checks** - gRPC health checking service

## Installation

```toml
[dependencies]
turbomcp-grpc = "3.0"
tokio = { version = "1", features = ["full"] }
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `server` | Enable server implementation | Yes |
| `client` | Enable client implementation | Yes |
| `health` | Enable gRPC health checking | No |
| `reflection` | Enable gRPC reflection | No |
| `tls` | Enable TLS support | No |

## Server

### Basic Server

```rust
use turbomcp_grpc::server::McpGrpcServer;
use turbomcp_core::types::Tool;

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

### McpGrpcServer Builder

```rust
use turbomcp_grpc::server::McpGrpcServer;

let server = McpGrpcServer::builder()
    // Server identification
    .server_info("name", "version")
    .instructions("Welcome to my server")

    // Add tools
    .add_tool(tool)
    .add_tools(vec![tool1, tool2])
    .tool_handler(|name, args| async move {
        // Handle tool calls
        Ok(json!({"result": "success"}))
    })

    // Add resources
    .add_resource(resource)
    .add_resources(vec![res1, res2])
    .resource_handler(|uri| async move {
        // Read resource
        Ok(contents)
    })

    // Add prompts
    .add_prompt(prompt)
    .prompt_handler(|name, args| async move {
        // Get prompt
        Ok(messages)
    })

    // Build
    .build();
```

### Server with TLS

```rust
use tonic::transport::{Server, ServerTlsConfig, Identity};

let cert = std::fs::read("server.pem")?;
let key = std::fs::read("server.key")?;
let identity = Identity::from_pem(cert, key);

let tls_config = ServerTlsConfig::new().identity(identity);

Server::builder()
    .tls_config(tls_config)?
    .add_service(server.into_service())
    .serve("[::1]:50051".parse()?)
    .await?;
```

### Health Checking

```rust
use tonic_health::server::health_reporter;

let (mut health_reporter, health_service) = health_reporter();
health_reporter
    .set_serving::<McpGrpcServer>()
    .await;

Server::builder()
    .add_service(health_service)
    .add_service(server.into_service())
    .serve("[::1]:50051".parse()?)
    .await?;
```

## Client

### Basic Client

```rust
use turbomcp_grpc::client::McpGrpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = McpGrpcClient::connect("http://[::1]:50051").await?;

    // Initialize session
    let init_result = client.initialize().await?;
    println!("Connected to: {:?}", init_result.server_info);

    // List tools
    let tools = client.list_tools().await?;
    println!("Available tools: {:?}", tools);

    // Call a tool
    let result = client.call_tool(
        "hello",
        Some(serde_json::json!({"name": "World"}))
    ).await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

### McpGrpcClient Methods

```rust
impl McpGrpcClient {
    // Connection
    pub async fn connect(endpoint: &str) -> Result<Self, GrpcError>;
    pub async fn connect_with_tls(endpoint: &str, tls: ClientTlsConfig) -> Result<Self, GrpcError>;

    // Session
    pub async fn initialize(&mut self) -> Result<InitializeResult, GrpcError>;
    pub fn is_initialized(&self) -> bool;
    pub async fn ping(&mut self) -> Result<(), GrpcError>;

    // Tools
    pub async fn list_tools(&mut self) -> Result<Vec<Tool>, GrpcError>;
    pub async fn call_tool(&mut self, name: &str, args: Option<Value>) -> Result<CallToolResult, GrpcError>;

    // Resources
    pub async fn list_resources(&mut self) -> Result<Vec<Resource>, GrpcError>;
    pub async fn read_resource(&mut self, uri: &str) -> Result<ReadResourceResult, GrpcError>;
    pub async fn list_resource_templates(&mut self) -> Result<Vec<ResourceTemplate>, GrpcError>;

    // Prompts
    pub async fn list_prompts(&mut self) -> Result<Vec<Prompt>, GrpcError>;
    pub async fn get_prompt(&mut self, name: &str, args: Option<Value>) -> Result<GetPromptResult, GrpcError>;

    // Completions
    pub async fn complete(&mut self, request: CompleteRequest) -> Result<CompleteResult, GrpcError>;

    // Logging
    pub async fn set_logging_level(&mut self, level: LoggingLevel) -> Result<(), GrpcError>;

    // Notifications
    pub async fn subscribe(&mut self) -> Result<impl Stream<Item = Notification>, GrpcError>;
}
```

### Client with TLS

```rust
use tonic::transport::ClientTlsConfig;

let tls_config = ClientTlsConfig::new()
    .ca_certificate(Certificate::from_pem(ca_cert));

let client = McpGrpcClient::connect_with_tls(
    "https://[::1]:50051",
    tls_config
).await?;
```

### Streaming Notifications

```rust
use futures::StreamExt;

let mut notifications = client.subscribe().await?;

while let Some(notification) = notifications.next().await {
    match notification? {
        Notification::ToolsListChanged => {
            let tools = client.list_tools().await?;
            println!("Tools updated: {:?}", tools);
        }
        Notification::ResourcesListChanged => {
            let resources = client.list_resources().await?;
            println!("Resources updated: {:?}", resources);
        }
        _ => {}
    }
}
```

## Tower Integration

### McpGrpcLayer

```rust
use turbomcp_grpc::layer::McpGrpcLayer;
use tower::ServiceBuilder;
use std::time::Duration;

let layer = McpGrpcLayer::new()
    .timeout(Duration::from_secs(30))
    .logging(true)
    .timing(true)
    .retry_count(3);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(inner_service);
```

### Layer Configuration

```rust
impl McpGrpcLayer {
    pub fn new() -> Self;

    /// Set request timeout
    pub fn timeout(self, duration: Duration) -> Self;

    /// Enable request/response logging
    pub fn logging(self, enabled: bool) -> Self;

    /// Enable timing metrics
    pub fn timing(self, enabled: bool) -> Self;

    /// Set retry count for failed requests
    pub fn retry_count(self, count: usize) -> Self;

    /// Set maximum concurrent requests
    pub fn concurrency_limit(self, limit: usize) -> Self;
}
```

## Protocol Definition

The gRPC service is defined in Protocol Buffers:

```protobuf
service McpService {
    // Session
    rpc Initialize(InitializeRequest) returns (InitializeResponse);
    rpc Ping(PingRequest) returns (PingResponse);

    // Tools
    rpc ListTools(ListToolsRequest) returns (ListToolsResponse);
    rpc CallTool(CallToolRequest) returns (CallToolResponse);

    // Resources
    rpc ListResources(ListResourcesRequest) returns (ListResourcesResponse);
    rpc ReadResource(ReadResourceRequest) returns (ReadResourceResponse);
    rpc ListResourceTemplates(ListResourceTemplatesRequest) returns (ListResourceTemplatesResponse);

    // Prompts
    rpc ListPrompts(ListPromptsRequest) returns (ListPromptsResponse);
    rpc GetPrompt(GetPromptRequest) returns (GetPromptResponse);

    // Completions
    rpc Complete(CompleteRequest) returns (CompleteResponse);

    // Logging
    rpc SetLoggingLevel(SetLoggingLevelRequest) returns (SetLoggingLevelResponse);

    // Notifications (server streaming)
    rpc Subscribe(SubscribeRequest) returns (stream Notification);
}
```

## Error Handling

### GrpcError

```rust
use turbomcp_grpc::error::GrpcError;

pub enum GrpcError {
    /// Connection failed
    ConnectionError(String),

    /// Request timed out
    Timeout,

    /// Server returned error status
    Status(tonic::Status),

    /// Protocol error
    ProtocolError(String),

    /// Not initialized
    NotInitialized,
}
```

### Error Handling Example

```rust
use turbomcp_grpc::{client::McpGrpcClient, error::GrpcError};

async fn safe_call(client: &mut McpGrpcClient) -> Result<(), GrpcError> {
    match client.call_tool("my_tool", None).await {
        Ok(result) => {
            println!("Success: {:?}", result);
            Ok(())
        }
        Err(GrpcError::Status(status)) => {
            eprintln!("Server error: {} - {}", status.code(), status.message());
            Err(GrpcError::Status(status))
        }
        Err(GrpcError::Timeout) => {
            eprintln!("Request timed out");
            Err(GrpcError::Timeout)
        }
        Err(e) => {
            eprintln!("Other error: {:?}", e);
            Err(e)
        }
    }
}
```

## Interceptors

Add request interceptors for authentication, logging, etc:

```rust
use tonic::{Request, Status};

fn auth_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let token = "Bearer my-token";
    req.metadata_mut().insert(
        "authorization",
        token.parse().unwrap()
    );
    Ok(req)
}

let channel = Channel::from_static("http://[::1]:50051")
    .connect()
    .await?;

let client = McpGrpcClient::with_interceptor(channel, auth_interceptor);
```

## Metrics

When using the Tower layer with timing enabled:

```rust
let layer = McpGrpcLayer::new().timing(true);

// Metrics are emitted via tracing
// mcp.grpc.request_duration_seconds
// mcp.grpc.requests_total
// mcp.grpc.errors_total
```

## Load Balancing

```rust
use tonic::transport::Channel;

let channel = Channel::balance_list(
    vec![
        "http://server1:50051".parse()?,
        "http://server2:50051".parse()?,
        "http://server3:50051".parse()?,
    ].into_iter()
);

let client = McpGrpcClient::from_channel(channel);
```

## Next Steps

- **[Tower Middleware Guide](../guide/tower-middleware.md)** - Middleware patterns
- **[Transports Guide](../guide/transports.md)** - All transport options
- **[Telemetry API](telemetry.md)** - Observability integration
