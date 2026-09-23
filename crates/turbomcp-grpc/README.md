# turbomcp-grpc

gRPC transport for the Model Context Protocol (MCP), built on tonic.

This crate defines its own gRPC service (`src/proto/mcp.proto`) that mirrors
MCP's methods. It is not a wire format other MCP SDKs speak: both ends need to
use this proto, typically `McpGrpcServer` on one side and `McpGrpcClient` (or
any client generated from the proto) on the other.

## What's included

- **Server** (`McpGrpcServer`): serves tools, resources, resource templates and
  prompts that you register, dispatching calls to your handlers, plus a
  server-streaming `Subscribe` for notifications
- **Client** (`McpGrpcClient`): a wrapper for the handshake and the tool,
  resource and prompt methods
- **Tower integration** (`McpGrpcLayer`): request logging and timing
- **TLS** (`tls` feature, on by default): rustls through tonic

## Requirements

Building this crate compiles `mcp.proto`, which needs the `protoc` Protocol
Buffers compiler on `PATH` (or named by the `PROTOC` environment variable):

- macOS: `brew install protobuf`
- Debian/Ubuntu: `apt-get install protobuf-compiler`
- Others: <https://github.com/protocolbuffers/protobuf/releases>

## Installation

```toml
[dependencies]
turbomcp-grpc = "3.5.0"
```

## Quick Start

### Server

```rust
use std::future::Future;
use std::pin::Pin;

use turbomcp_grpc::server::ToolHandler;
use turbomcp_grpc::{GrpcResult, McpGrpcServer};
use turbomcp_protocol::types::CallToolResult;
use turbomcp_types::{Tool, ToolInputSchema};

struct Hello;

impl ToolHandler for Hello {
    fn call_tool(
        &self,
        _name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Pin<Box<dyn Future<Output = GrpcResult<CallToolResult>> + Send + '_>> {
        let who = arguments
            .as_ref()
            .and_then(|args| args["name"].as_str())
            .unwrap_or("world")
            .to_string();
        Box::pin(async move { Ok(CallToolResult::text(format!("Hello, {who}!"))) })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpGrpcServer::builder()
        .server_info("my-server", "1.0.0")
        .add_tool(
            Tool::new("hello", "Says hello").with_schema(ToolInputSchema::from_value(
                serde_json::json!({
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                }),
            )),
        )
        .tool_handler(Hello)
        .build();

    tonic::transport::Server::builder()
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;

    Ok(())
}
```

A tool with no handler registered answers every call with an error.

### Client

```rust
use turbomcp_grpc::McpGrpcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = McpGrpcClient::connect("http://[::1]:50051").await?;

    let init = client.initialize().await?;
    println!("Connected to {} ({})", init.server_info.name, init.protocol_version);

    let tools = client.list_tools().await?;
    println!("Available tools: {tools:?}");

    let result = client
        .call_tool("hello", Some(serde_json::json!({ "name": "World" })))
        .await?;
    println!("Result: {result:?}");

    Ok(())
}
```

## Protocol

`mcp.proto` (package `turbomcp.mcp.v1`) defines the RPCs below. The last column
is what `McpGrpcServer` does with each.

| RPC | `McpGrpcServer` | `McpGrpcClient` method |
|-----|-----------------|------------------------|
| `Initialize` | Negotiates the protocol version (below) | `initialize` |
| `Ping` | Answers | `ping` |
| `ListTools` / `CallTool` | Registered tools, your `ToolHandler` | `list_tools` / `call_tool` |
| `ListResources` / `ListResourceTemplates` / `ReadResource` | Registered resources, your `ResourceHandler` | `list_resources` / `list_resource_templates` / `read_resource` |
| `ListPrompts` / `GetPrompt` | Registered prompts, your `PromptHandler` | `list_prompts` / `get_prompt` |
| `Subscribe` | Streams every notification sent through the server; `topics` is ignored | none |
| `Complete` | Always returns no suggestions | none |
| `SetLoggingLevel` | Accepts and ignores the level | none |
| `ListRoots` | Always returns no roots | none |
| `CreateSamplingMessage` / `Elicit` | `UNIMPLEMENTED` | none |

List RPCs return everything in a single page. Roots, sampling and elicitation
are requests a *server* makes of a *client* in MCP; the proto's unary RPCs
point the other way, so there is no way to use them for that today. For the
RPCs the wrapper does not cover, use the generated client,
`turbomcp_grpc::proto::mcp_service_client::McpServiceClient`, directly.

### Version negotiation

`Initialize` follows the MCP lifecycle rules. The server echoes the client's
requested version when it is one this SDK supports (`2025-06-18` or
`2025-11-25`), and otherwise answers with the builder's `protocol_version`
(default: the latest). The client records the version the server chose and
fails `initialize` if it is one the SDK does not support.

### Metadata

Tool annotations, `outputSchema`, `execution`, `structuredContent`, icons
(including `mimeType`, `sizes` and `theme`), `resource_link` content blocks and
`_meta` all cross the wire in both directions. Free-form JSON (schemas,
structured content, `_meta`) is carried as JSON-encoded `bytes` fields.

## Tower Integration

```rust
use tower::ServiceBuilder;
use turbomcp_grpc::McpGrpcLayer;

let layer = McpGrpcLayer::new()
    .logging(true)
    .timing(true);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(inner_service);
```

## TLS

With the `tls` feature, configure the server through tonic:
`tonic::transport::Server::builder().tls_config(...)`. `McpGrpcClient`
connects to `https://` endpoints and verifies them against the platform's root
certificates. It does not take a custom CA or client certificate; for those,
build a `tonic::transport::Channel` yourself and use the generated client.

## Features

- `server` (default): `McpGrpcServer`
- `client` (default): `McpGrpcClient`
- `tls` (default): rustls TLS for tonic, with platform root certificates for
  clients

## License

MIT
