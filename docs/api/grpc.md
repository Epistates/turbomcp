# gRPC Transport API Reference

The `turbomcp-grpc` crate provides a tonic-based gRPC transport for MCP. It
exposes a server service, a client wrapper, and a small Tower layer for request
logging/timing.

## Installation

```toml
[dependencies]
turbomcp-grpc = "3.2.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tonic = "0.14"
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `server` | Build `McpGrpcServer` | Yes |
| `client` | Build `McpGrpcClient` | Yes |
| `health` | Compatibility feature; use MCP `Ping` or add `tonic-health` directly | No |
| `reflection` | Reserved compatibility feature | No |
| `tls` | Reserved compatibility feature; TLS is configured through tonic | No |

## Server

```rust
use tonic::transport::Server;
use turbomcp_grpc::McpGrpcServer;
use turbomcp_types::{Tool, ToolInputSchema};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpGrpcServer::builder()
        .server_info("my-server", "1.0.0")
        .add_tool(
            Tool::new("hello", "Says hello").with_schema(
                ToolInputSchema::default()
                    .add_property("name", serde_json::json!({"type": "string"}))
                    .require_property("name"),
            ),
        )
        .build();

    Server::builder()
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;

    Ok(())
}
```

### Builder Surface

```rust
use turbomcp_grpc::server::McpGrpcServer;

let server = McpGrpcServer::builder()
    .server_info("name", "version")
    .protocol_version("2025-11-25")
    .instructions("Welcome")
    .capabilities(server_capabilities)
    .add_tool(tool)
    .add_resource(resource)
    .add_resource_template(template)
    .add_prompt(prompt)
    .tool_handler(my_tool_handler)
    .resource_handler(my_resource_handler)
    .prompt_handler(my_prompt_handler)
    .build();
```

Handlers are trait implementations: `ToolHandler`, `ResourceHandler`, and
`PromptHandler`.

### Server TLS

TLS is configured on tonic's `Server` builder:

```rust
use tonic::transport::{Identity, Server, ServerTlsConfig};

let cert = std::fs::read("server.pem")?;
let key = std::fs::read("server.key")?;
let identity = Identity::from_pem(cert, key);

Server::builder()
    .tls_config(ServerTlsConfig::new().identity(identity))?
    .add_service(server.into_service())
    .serve("[::1]:50051".parse()?)
    .await?;
```

## Client

```rust
use turbomcp_grpc::McpGrpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = McpGrpcClient::connect("http://[::1]:50051").await?;

    let init_result = client.initialize().await?;
    println!("Connected to: {:?}", init_result.server_info);

    let tools = client.list_tools().await?;
    println!("Available tools: {:?}", tools);

    let result = client
        .call_tool("hello", Some(serde_json::json!({"name": "World"})))
        .await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

### Client Configuration

```rust
use std::time::Duration;
use turbomcp_grpc::client::{McpGrpcClient, McpGrpcClientConfig};

let config = McpGrpcClientConfig {
    name: "my-client".to_string(),
    version: "1.0.0".to_string(),
    connect_timeout: Duration::from_secs(5),
    request_timeout: Duration::from_secs(30),
    ..Default::default()
};

let client = McpGrpcClient::connect_with_config("http://[::1]:50051", config).await?;
```

### Client Methods

```rust
impl McpGrpcClient {
    pub async fn connect(addr: impl AsRef<str>) -> GrpcResult<Self>;
    pub async fn connect_with_config(
        addr: impl AsRef<str>,
        config: McpGrpcClientConfig,
    ) -> GrpcResult<Self>;

    pub async fn initialize(&mut self) -> GrpcResult<InitializeResult>;
    pub async fn ping(&mut self) -> GrpcResult<()>;

    pub async fn list_tools(&mut self) -> GrpcResult<Vec<Tool>>;
    pub async fn call_tool(
        &mut self,
        name: impl AsRef<str>,
        arguments: Option<serde_json::Value>,
    ) -> GrpcResult<CallToolResult>;

    pub async fn list_resources(&mut self) -> GrpcResult<Vec<Resource>>;
    pub async fn list_resource_templates(&mut self) -> GrpcResult<Vec<ResourceTemplate>>;
    pub async fn read_resource(&mut self, uri: impl AsRef<str>) -> GrpcResult<Vec<ResourceContent>>;

    pub async fn list_prompts(&mut self) -> GrpcResult<Vec<Prompt>>;
    pub async fn get_prompt(
        &mut self,
        name: impl AsRef<str>,
        arguments: Option<serde_json::Value>,
    ) -> GrpcResult<GetPromptResult>;

    pub fn server_info(&self) -> Option<&Implementation>;
    pub fn server_capabilities(&self) -> Option<&ServerCapabilities>;
    pub fn protocol_version(&self) -> &str;
}
```

Client TLS is configured through tonic's `Endpoint`/`Channel` APIs today. The
`McpGrpcClient` convenience constructor accepts an endpoint URL and applies
timeout settings from `McpGrpcClientConfig`.

## Tower Integration

```rust
use tower::ServiceBuilder;
use turbomcp_grpc::McpGrpcLayer;

let service = ServiceBuilder::new()
    .layer(McpGrpcLayer::new().logging(true).timing(true))
    .service(inner_service);
```

`McpGrpcLayer` exposes:

```rust
impl McpGrpcLayer {
    pub fn new() -> Self;
    pub fn logging(self, enabled: bool) -> Self;
    pub fn timing(self, enabled: bool) -> Self;
}
```

## Error Handling

```rust
use turbomcp_grpc::{GrpcError, McpGrpcClient};

async fn safe_call(client: &mut McpGrpcClient) -> Result<(), GrpcError> {
    match client.call_tool("my_tool", None).await {
        Ok(result) => {
            println!("Success: {:?}", result);
            Ok(())
        }
        Err(GrpcError::Status(status)) => {
            eprintln!("gRPC status: {} - {}", status.code(), status.message());
            Err(GrpcError::Status(status))
        }
        Err(e) => Err(e),
    }
}
```

## Protocol Definition

The generated gRPC service is defined in
`crates/turbomcp-grpc/src/proto/mcp.proto`.

## Next Steps

- [Tower Middleware Guide](../guide/tower-middleware.md)
- [Transports Guide](../guide/transports.md)
- [Telemetry API](telemetry.md)
