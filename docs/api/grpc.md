# gRPC Transport API Reference

The `turbomcp-grpc` crate provides a tonic-based gRPC transport for MCP. It
exposes a server service, a client wrapper, and a small Tower layer for request
logging/timing. It is independent of the `#[server]` macro: a gRPC server is
assembled from explicit tool/resource/prompt lists and handler traits.

## Installation

```toml
[dependencies]
turbomcp-grpc = "3.5.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tonic = "0.14"
serde_json = "1"
```

Building the crate compiles `src/proto/mcp.proto`, which needs the `protoc`
compiler on `PATH` (or named by the `PROTOC` environment variable).

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `server` | Build `McpGrpcServer` | Yes |
| `client` | Build `McpGrpcClient` | Yes |
| `tls` | rustls for tonic: `Server::tls_config` on the server, `https://` endpoints verified against the platform roots on the client | Yes |

## Server

The server answers `tools/list`, `resources/list` and `prompts/list` from the
lists registered on the builder, and dispatches calls to the handler traits.
Capabilities are not inferred from what is registered: set them with
`.capabilities(...)`, or `initialize` advertises none.

```rust
use std::future::Future;
use std::pin::Pin;
use tonic::transport::Server;
use turbomcp_grpc::server::ToolHandler;
use turbomcp_grpc::{GrpcError, GrpcResult, McpGrpcServer};
use turbomcp_protocol::capabilities::builders::ServerCapabilitiesBuilder;
use turbomcp_protocol::types::CallToolResult;
use turbomcp_types::{Tool, ToolInputSchema};

struct Hello;

impl ToolHandler for Hello {
    fn call_tool(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Pin<Box<dyn Future<Output = GrpcResult<CallToolResult>> + Send + '_>> {
        let name = name.to_string();
        Box::pin(async move {
            match name.as_str() {
                "hello" => {
                    let who = arguments
                        .as_ref()
                        .and_then(|args| args["name"].as_str())
                        .unwrap_or("World");
                    Ok(CallToolResult::text(format!("Hello, {who}!")))
                }
                other => Err(GrpcError::invalid_request(format!("unknown tool: {other}"))),
            }
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpGrpcServer::builder()
        .server_info("my-server", "1.0.0")
        .capabilities(ServerCapabilitiesBuilder::new().enable_tools().build())
        .add_tool(
            Tool::new("hello", "Says hello").with_schema(
                ToolInputSchema::default()
                    .add_property("name", serde_json::json!({"type": "string"}))
                    .require_property("name"),
            ),
        )
        .tool_handler(Hello)
        .build();

    Server::builder()
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;

    Ok(())
}
```

### Builder Surface

`McpGrpcServer::builder()` returns a `McpGrpcServerBuilder` with these methods:

| Method | Purpose |
|---|---|
| `server_info(name, version)` | Implementation name and version |
| `protocol_version(version)` | Version offered to a client that requests an unsupported one (a supported request is echoed back) |
| `instructions(text)` | `instructions` in the `initialize` result |
| `capabilities(ServerCapabilities)` | Capabilities advertised in `initialize` |
| `add_tool` / `add_resource` / `add_resource_template` / `add_prompt` | Entries returned by the list methods |
| `tool_handler` / `resource_handler` / `prompt_handler` | Implementations of `ToolHandler`, `ResourceHandler`, `PromptHandler` |
| `build()` | Produce the `McpGrpcServer`; `into_service()` turns it into a tonic service |

A handler that is not set answers every call with an error. The built server
can push list-changed notifications with `notify_tool_list_changed()`,
`notify_resource_list_changed()`, and `notify_prompt_list_changed()`.

### Server TLS

TLS (the default `tls` feature) is configured on tonic's `Server` builder:

```rust
use tonic::transport::{Identity, Server, ServerTlsConfig};
use turbomcp_grpc::McpGrpcServer;

async fn serve_tls(server: McpGrpcServer) -> Result<(), Box<dyn std::error::Error>> {
    let cert = std::fs::read("server.pem")?;
    let key = std::fs::read("server.key")?;
    let identity = Identity::from_pem(cert, key);

    Server::builder()
        .tls_config(ServerTlsConfig::new().identity(identity))?
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;
    Ok(())
}
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

async fn connect() -> Result<McpGrpcClient, turbomcp_grpc::GrpcError> {
    let config = McpGrpcClientConfig {
        name: "my-client".to_string(),
        version: "1.0.0".to_string(),
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(30),
        ..Default::default()
    };

    McpGrpcClient::connect_with_config("http://[::1]:50051", config).await
}
```

`McpGrpcClientConfig` also has `protocol_version` (the version requested in
`initialize`; the negotiated one replaces it) and `capabilities` (the
`ClientCapabilities` advertised).

### Client Methods

The client's methods, as signatures (this block is a listing, not code to compile):

```rust,ignore
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

The client connects with a plain URL. With the `tls` feature, an `https://`
endpoint is verified against the platform's root certificates; for anything
else (client certificates, a custom CA) build a tonic `Endpoint` yourself and
use the generated `turbomcp_grpc::proto::mcp_service_client::McpServiceClient`.

## Tower Integration

`McpGrpcLayer` logs and times each HTTP request of a tonic service:

```rust
use tonic::transport::Server;
use turbomcp_grpc::{McpGrpcLayer, McpGrpcServer};

async fn serve_with_layer(server: McpGrpcServer) -> Result<(), Box<dyn std::error::Error>> {
    Server::builder()
        .layer(McpGrpcLayer::new().logging(true).timing(true))
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;
    Ok(())
}
```

`McpGrpcLayer` exposes `new()` (logging and timing on), `logging(bool)`, and
`timing(bool)`. `turbomcp_grpc::layer::MetadataInterceptor` builds a tonic
interceptor that adds fixed metadata to every request.

## Error Handling

`GrpcError` separates transport failures from errors the server returned. A
tonic status from an MCP call is converted back into the MCP error it carries,
so it arrives as `GrpcError::Mcp`:

```rust
use turbomcp_grpc::{GrpcError, McpGrpcClient};

async fn safe_call(client: &mut McpGrpcClient) -> Result<(), GrpcError> {
    match client.call_tool("my_tool", None).await {
        Ok(result) => {
            println!("Success: {:?}", result);
            Ok(())
        }
        Err(GrpcError::Mcp(error)) => {
            eprintln!("server error ({:?}): {error}", error.kind);
            Err(GrpcError::Mcp(error))
        }
        Err(GrpcError::Transport(error)) => {
            eprintln!("connection failed: {error}");
            Err(GrpcError::Transport(error))
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
