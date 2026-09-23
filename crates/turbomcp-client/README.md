# TurboMCP Client

[![Crates.io](https://img.shields.io/crates/v/turbomcp-client.svg)](https://crates.io/crates/turbomcp-client)
[![Documentation](https://docs.rs/turbomcp-client/badge.svg)](https://docs.rs/turbomcp-client)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

MCP client for MCP `2025-11-25` and `2025-06-18` with bidirectional protocol support.

## Table of Contents

- [Overview](#overview)
- [Supported Transports](#supported-transports)
- [Quick Start](#quick-start)
- [Transport Configuration](#transport-configuration)
- [Advanced Features](#advanced-features)
- [Tower Middleware](#tower-middleware)
- [Sampling Handler Integration](#sampling-handler-integration)
- [Handler Registration](#handler-registration)
- [MCP Operations](#mcp-operations)
- [Error Handling](#error-handling)

## Overview

`turbomcp-client` provides a comprehensive MCP client implementation with:
- ✅ **Full MCP 2025-11-25 support** - Current server and client features
- ✅ **Bidirectional communication** - Server-initiated requests (sampling, elicitation, roots)
- ✅ **URL mode elicitation** - Opt in with `enable_elicitation_url()`
- ✅ **HTTP authorization** - Answer a server's `401`/`403` challenge with an `AuthProvider`
- ✅ **Per-call timeouts** - `client.with_timeout(duration)` for one slow request
- ✅ **Transport agnostic** - Works with STDIO, HTTP, TCP, Unix, WebSocket transports
- ✅ **Thread-safe sharing** - Client is cheaply cloneable via Arc for concurrent async tasks

## Supported Transports

| Transport | Status | Feature Flag | Use Case |
|-----------|--------|--------------|----------|
| **STDIO** | ✅ Full | default | Local process communication |
| **Streamable HTTP** | ✅ Full | `http` | HTTP POST + SSE client transport |
| **TCP** | ✅ Full | `tcp` | Network socket communication |
| **Unix** | ✅ Full | `unix` | Fast local IPC |
| **WebSocket** | ✅ Full | `websocket` | Real-time bidirectional |

`turbomcp-client` re-exports each enabled transport (`turbomcp_client::StdioTransport`,
`TcpTransport`, `StreamableHttpClientConfig`, …), so it is the only dependency the
examples below need besides `tokio` and `serde_json`.

## Quick Start

### Basic Client (STDIO)

```rust
use std::collections::HashMap;
use turbomcp_client::{Client, StdioTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // Create client with STDIO transport
    let client = Client::new(StdioTransport::new());

    // Initialize connection
    let result = client.initialize().await?;
    println!("Connected to: {}", result.server_info.name);

    // List and call tools
    let tools = client.list_tools().await?;
    for tool in &tools {
        println!("Tool: {} - {}", tool.name,
            tool.description.as_deref().unwrap_or("No description"));
    }

    // Call a tool: name, arguments, optional task augmentation
    let result = client.call_tool(
        "calculator",
        Some(HashMap::from([
            ("operation".to_string(), serde_json::json!("add")),
            ("a".to_string(), serde_json::json!(5)),
            ("b".to_string(), serde_json::json!(3)),
        ])),
        None,
    ).await?;

    println!("Result: {:?}", result);
    client.shutdown().await?;
    Ok(())
}
```

### HTTP Client (One-Liner)

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // Connects to http://localhost:8080/mcp and initializes
    let client = Client::connect_http("http://localhost:8080").await?;

    // Ready to use immediately
    let tools = client.list_tools().await?;
    println!("Found {} tools", tools.len());

    Ok(())
}
```

### TCP/Unix Clients

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // TCP (feature `tcp`)
    let tcp = Client::connect_tcp("127.0.0.1:8765").await?;

    // Unix socket (feature `unix`)
    let unix = Client::connect_unix("/tmp/mcp.sock").await?;
    Ok(())
}
```

### With ClientBuilder

```rust
use turbomcp_client::{ClientBuilder, StdioTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = ClientBuilder::new()
        .with_tools(true)
        .with_prompts(true)
        .with_resources(true)
        .with_sampling(false)
        .with_timeout(30_000) // request timeout, in milliseconds
        .build(StdioTransport::new())
        .await?;

    // build() does not connect: initialize before use
    client.initialize().await?;
    Ok(())
}
```

### Cloning Client for Concurrent Usage

```rust
use turbomcp_client::{Client, StdioTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client (cheaply cloneable via Arc)
    let client = Client::new(StdioTransport::new());

    // Initialize once
    client.initialize().await?;

    // Clone for multiple async tasks - this is cheap (just Arc clone)
    let client1 = client.clone();
    let client2 = client.clone();

    let handle1 = tokio::spawn(async move { client1.list_tools().await });
    let handle2 = tokio::spawn(async move { client2.list_prompts().await });

    let (tools, prompts) = tokio::try_join!(handle1, handle2)?;
    Ok(())
}
```

## Transport Configuration

### STDIO Transport (Default)

```rust
use turbomcp_client::{Client, StdioTransport};

let client = Client::new(StdioTransport::new());
```

### HTTP Transport

```rust
use std::time::Duration;
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // Defaults: endpoint path /mcp, 30 s timeout
    let client = Client::connect_http("http://localhost:8080").await?;

    // Or adjust the StreamableHttpClientConfig first
    let client = Client::connect_http_with("http://localhost:8080", |config| {
        config.timeout = Duration::from_secs(60);
        config.endpoint_path = "/api/mcp".to_string();
    }).await?;
    Ok(())
}
```

A server that requires authorization answers a request without a usable token
`401` (or `403` for a missing scope) with a `WWW-Authenticate` challenge. Set
`auth_provider` to an `AuthProvider` from `turbomcp-http` to supply the token
and to handle the challenge; when it reports a new token, the request is
retried once:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp_client::Client;
use turbomcp_http::{AuthChallenge, AuthFuture, AuthProvider};

#[derive(Debug, Default)]
struct MyTokens {
    token: RwLock<Option<String>>,
}

impl AuthProvider for MyTokens {
    fn token(&self) -> AuthFuture<'_, Option<String>> {
        Box::pin(async move { self.token.read().await.clone() })
    }

    fn on_challenge<'a>(&'a self, challenge: &'a AuthChallenge) -> AuthFuture<'a, bool> {
        Box::pin(async move {
            // `challenge.resource_metadata` names the server's Protected Resource
            // Metadata: the starting point for discovering its authorization
            // server (see turbomcp_auth::discovery) and getting a token.
            eprintln!("authorization required: {challenge}");
            false // no new token, so the request fails
        })
    }
}

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::connect_http_with("https://mcp.example.com", |config| {
        config.auth_provider = Some(Arc::new(MyTokens::default()));
    }).await?;
    Ok(())
}
```

### TCP Transport

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // One-liner - connects and initializes automatically
    let client = Client::connect_tcp("127.0.0.1:8765").await?;
    Ok(())
}
```

Or using transport directly:

```rust
use std::net::SocketAddr;
use turbomcp_client::{Client, TcpTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_addr: SocketAddr = "127.0.0.1:8765".parse()?;
    let bind_addr: SocketAddr = "0.0.0.0:0".parse()?;  // Any available port
    let transport = TcpTransport::new_client(bind_addr, server_addr);
    let client = Client::new(transport);
    client.initialize().await?;
    Ok(())
}
```

### Unix Socket Transport

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    // One-liner - connects and initializes automatically
    let client = Client::connect_unix("/tmp/mcp.sock").await?;
    Ok(())
}
```

Or using transport directly:

```rust
use std::path::PathBuf;
use turbomcp_client::{Client, UnixTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let transport = UnixTransport::new_client(PathBuf::from("/tmp/mcp.sock"));
    let client = Client::new(transport);
    client.initialize().await?;
    Ok(())
}
```

### WebSocket Transport

```rust
use turbomcp_client::{Client, WebSocketBidirectionalConfig, WebSocketBidirectionalTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WebSocketBidirectionalConfig {
        url: Some("ws://localhost:8080".to_string()),
        ..Default::default()
    };

    let transport = WebSocketBidirectionalTransport::new(config).await?;
    let client = Client::new(transport);
    client.initialize().await?;
    Ok(())
}
```

## Advanced Features

### Per-Call Timeouts

`with_timeout` returns a handle to the same client that uses a different
request timeout. When a request times out, the server is sent
`notifications/cancelled` for it:

```rust
use std::time::Duration;
use turbomcp_client::{Client, StdioTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::new(StdioTransport::new());
    client.initialize().await?;

    // A slow tool gets ten minutes; everything else keeps the default.
    let result = client
        .with_timeout(Duration::from_secs(600))
        .call_tool("reindex", None, None)
        .await?;
    Ok(())
}
```

### Robust Transport with Retry & Circuit Breaker

`build_resilient` wraps the transport with retry, a circuit breaker, and
health checking. (`build()` refuses resilience settings rather than ignore
them.)

```rust
use std::time::Duration;
use turbomcp_client::{ClientBuilder, StdioTransport};
use turbomcp_transport::resilience::{CircuitBreakerConfig, RetryConfig};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = ClientBuilder::new()
        .with_retry_config(RetryConfig {
            max_attempts: 5,
            base_delay: Duration::from_millis(200),
            ..Default::default()
        })
        .with_circuit_breaker_config(CircuitBreakerConfig {
            failure_threshold: 3,
            timeout: Duration::from_secs(30),
            ..Default::default()
        })
        .build_resilient(StdioTransport::new())
        .await?;
    client.initialize().await?;
    Ok(())
}
```

### Tower Middleware

The v2.x plugin system has been replaced by Tower layers in
`turbomcp_client::middleware`. Each wraps a
`Service<McpRequest, Response = McpResponse>`; the `Client` does not route its
own requests through a Tower stack, so compose them around a service of your
own:

```rust
use std::sync::Arc;
use tower::ServiceBuilder;
use turbomcp_client::middleware::{
    CacheLayer, McpRequest, McpResponse, Metrics, MetricsLayer, TracingLayer,
};

let inner = tower::service_fn(|request: McpRequest| async move {
    // Send `request.request` (a JSON-RPC request) to a server here.
    Ok::<_, turbomcp_client::Error>(McpResponse::success(serde_json::json!({}), request.elapsed()))
});

// Keep a handle to read the counters later with `metrics.snapshot()`
let metrics = Arc::new(Metrics::new());
let service = ServiceBuilder::new()
    .layer(TracingLayer::new())
    .layer(MetricsLayer::new(metrics.clone()))
    .layer(CacheLayer::default())
    .service(inner);
```

See [MIGRATION.md](../../MIGRATION.md) for the full v2 → v3 migration.

### Sampling Handler Integration

Handle server-initiated sampling requests by implementing a custom sampling handler:

```rust
use std::sync::Arc;
use turbomcp_client::sampling::{BoxSamplingFuture, SamplingHandler};
use turbomcp_client::{Client, StdioTransport};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, Role, SamplingContent, StopReason,
};

#[derive(Debug)]
struct MySamplingHandler {
    // Your LLM integration (OpenAI, Anthropic, local model, etc.)
}

impl SamplingHandler for MySamplingHandler {
    fn handle_create_message(
        &self,
        _request_id: String,
        _request: CreateMessageRequest,
    ) -> BoxSamplingFuture<'_, CreateMessageResult> {
        Box::pin(async move {
            // Forward to your LLM service.
            // Use request_id for correlation/tracking.
            Ok(CreateMessageResult {
                role: Role::Assistant,
                content: SamplingContent::text("Generated response").into(),
                model: "your-model".to_string(),
                stop_reason: Some(StopReason::EndTurn.to_string()),
                meta: None,
            })
        })
    }
}

let client = Client::new(StdioTransport::new());
client.set_sampling_handler(Arc::new(MySamplingHandler { /* ... */ }));
```

**Note:** TurboMCP provides the sampling protocol infrastructure. You implement your own LLM integration (OpenAI SDK, Anthropic SDK, local models, etc.) as needed for your use case.

### Handler Registration

```rust
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use turbomcp_client::handlers::{
    ElicitationHandler, ElicitationRequest, ElicitationResponse, HandlerResult,
};
use turbomcp_client::{ClientBuilder, StdioTransport};

#[derive(Debug)]
struct MyElicitationHandler;

impl ElicitationHandler for MyElicitationHandler {
    fn handle_elicitation(
        &self,
        request: ElicitationRequest,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<ElicitationResponse>> + Send + '_>> {
        Box::pin(async move {
            // Prompt the user for input based on `request.schema()`.
            let mut content = HashMap::new();
            content.insert("name".to_string(), serde_json::json!("Alice"));
            Ok(ElicitationResponse::accept(content))
        })
    }
}

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = ClientBuilder::new()
        .with_elicitation_handler(Arc::new(MyElicitationHandler))
        .build(StdioTransport::new())
        .await?;
    client.initialize().await?;
    Ok(())
}
```

`ElicitationResponse` has four constructors: `accept(content)`, `accept_without_content()`,
`decline()`, and `cancel()`. The response fields are private — do not build it as a struct literal.

#### URL mode elicitation

A server may ask the user to visit a URL (for example to authorize a third
party) instead of filling in a form. Servers only send URL mode to a client
that declared it, so opt in with `enable_elicitation_url()` before
`initialize()`, and only if your handler shows the URL to the user — with
their consent, never by opening it automatically. Answer with
`accept_without_content()` once they have seen it. The server later sends
`notifications/elicitation/complete`, delivered to an
`ElicitationCompleteHandler`:

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use turbomcp_client::handlers::{
    ElicitationCompleteHandler, ElicitationHandler, ElicitationRequest, ElicitationResponse,
    HandlerResult,
};
use turbomcp_client::{Client, StdioTransport};
use turbomcp_protocol::types::ElicitRequestParams;

#[derive(Debug)]
struct ShowUrl;

impl ElicitationHandler for ShowUrl {
    fn handle_elicitation(
        &self,
        request: ElicitationRequest,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<ElicitationResponse>> + Send + '_>> {
        Box::pin(async move {
            match request.as_protocol() {
                ElicitRequestParams::Url(params) => {
                    // Show the URL and ask for consent; never open it automatically.
                    eprintln!("{}: {}", params.message, params.url);
                    Ok(ElicitationResponse::accept_without_content())
                }
                ElicitRequestParams::Form(_) => Ok(ElicitationResponse::decline()),
            }
        })
    }
}

#[derive(Debug)]
struct Finished;

impl ElicitationCompleteHandler for Finished {
    fn handle_elicitation_complete(
        &self,
        elicitation_id: String,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<()>> + Send + '_>> {
        Box::pin(async move {
            // Typically the cue to retry the request that needed it
            eprintln!("elicitation {elicitation_id} finished");
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::new(StdioTransport::new());
    client.set_elicitation_handler(Arc::new(ShowUrl));
    client.enable_elicitation_url();
    client.set_elicitation_complete_handler(Arc::new(Finished));
    client.initialize().await?;
    Ok(())
}
```

## MCP Operations

Each example below is a function over an initialized client on any transport.

### Tools

```rust
use std::collections::HashMap;
use turbomcp_client::{Client, Transport};

async fn tools(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // List available tools (follows pagination cursors)
    let tools = client.list_tools().await?;
    for tool in &tools {
        println!("{}: {}", tool.name, tool.description.as_deref().unwrap_or(""));
    }

    // List tool names only
    let names = client.list_tool_names().await?;

    // Call a tool
    let mut args = HashMap::new();
    args.insert("text".to_string(), serde_json::json!("Hello, world!"));
    let result = client.call_tool("echo", Some(args), None).await?;
    Ok(())
}
```

### Prompts

```rust
use turbomcp_client::{Client, Transport};
use turbomcp_protocol::types::PromptInput;

async fn prompts(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // List prompts
    let prompts = client.list_prompts().await?;

    // Get prompt with arguments.
    // `PromptInput` is a type alias for `HashMap<String, serde_json::Value>`.
    let mut prompt_args: PromptInput = PromptInput::new();
    prompt_args.insert("language".to_string(), serde_json::json!("rust"));
    prompt_args.insert("topic".to_string(), serde_json::json!("async programming"));

    let result = client.get_prompt("code_review", Some(prompt_args)).await?;
    println!("Prompt: {}", result.description.unwrap_or_default());
    for message in result.messages {
        println!("{:?}: {:?}", message.role, message.content);
    }
    Ok(())
}
```

### Resources

```rust
use turbomcp_client::{Client, Transport};

async fn resources(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // List resources
    let resources = client.list_resources().await?;

    // Read a resource
    let content = client.read_resource("file:///etc/hosts").await?;

    // List resource templates
    let templates = client.list_resource_templates().await?;
    Ok(())
}
```

### Completions

```rust
use std::collections::HashMap;
use turbomcp_client::{Client, Transport};
use turbomcp_protocol::types::CompletionContext;

async fn completions(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // Complete a prompt argument
    let completions = client.complete_prompt(
        "code_review",
        "framework",
        "tok",  // Partial input
        None,
    ).await?;

    for value in completions.completion.values {
        println!("Suggestion: {}", value);
    }

    // Complete with previously resolved arguments as context
    let context = CompletionContext {
        arguments: Some(HashMap::from([("language".to_string(), "rust".to_string())])),
    };
    let completions = client.complete_prompt("code_review", "framework", "tok", Some(context)).await?;
    Ok(())
}
```

### Subscriptions and Logging

```rust
use turbomcp_client::{Client, LogLevel, Transport};

async fn subscriptions(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // Subscribe to resource updates
    client.subscribe("file:///config.json").await?;

    // Set the level of the log messages the server sends
    client.set_log_level(LogLevel::Debug).await?;

    // Unsubscribe
    client.unsubscribe("file:///config.json").await?;
    Ok(())
}
```

### Health Monitoring

```rust
use turbomcp_client::{Client, Transport};

async fn health(client: &Client<impl Transport + 'static>) -> turbomcp_client::Result<()> {
    // Send ping to check connection
    let ping_result = client.ping().await?;
    println!("Server responded: {:?}", ping_result);
    Ok(())
}
```

## Bidirectional Communication

### Processing Server-Initiated Requests

Message processing is automatic: a dispatcher task routes server-initiated
requests (sampling, elicitation, roots) and notifications to the registered
handlers while your code uses the client. No manual message loop is needed.

```rust
use turbomcp_client::{Client, StdioTransport};

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let client = Client::new(StdioTransport::new());
    client.initialize().await?;

    // Bidirectional communication works automatically
    let tools = client.list_tools().await?;
    Ok(())
}
```

## Error Handling

`turbomcp_client::Error` is a re-export of `turbomcp_protocol::Error` (alias for
`turbomcp_core::McpError`). Errors are a struct with a classification (`ErrorKind`)
and a message, not an enum of variants — inspect `err.kind` / helpers rather than
pattern-matching variants:

```rust
use turbomcp_client::{Client, Transport};
use turbomcp_core::error::ErrorKind;

async fn call(client: &Client<impl Transport + 'static>) {
    match client.call_tool("my_tool", None, None).await {
        Ok(result) => println!("Success: {:?}", result),
        Err(err) => match err.kind {
            ErrorKind::Transport => eprintln!("Transport error: {err}"),
            ErrorKind::ProtocolVersionMismatch => eprintln!("Protocol mismatch: {err}"),
            _ if err.is_retryable() => eprintln!("Retryable error: {err}"),
            _ => eprintln!("Error ({:?}): {err}", err.kind),
        },
    }
}
```

A tool that ran and failed is not an `Err`: it returns `Ok` with
`result.is_error == Some(true)`, and the server's error kind in `_meta`.

## Examples

For working client examples, see the parent `turbomcp` crate examples directory
(`crates/turbomcp/examples/`). Client-oriented examples include:

- **`tcp_client.rs`** — TCP transport client
- **`unix_client.rs`** — Unix socket client
- **`test_client.rs`** — programmatic test client

Run examples from the workspace root:
```bash
cargo run -p turbomcp --example tcp_client --features tcp,full-client
cargo run -p turbomcp --example unix_client --features unix,full-client
```

## Feature Flags

| Feature | Description | Status |
|---------|-------------|--------|
| `default` | STDIO transport only | ✅ |
| `tcp` | TCP transport | ✅ |
| `unix` | Unix socket transport | ✅ |
| `websocket` | WebSocket transport | ✅ |
| `http` | Streamable HTTP client transport | ✅ |
| `experimental-tasks` | Task-augmented requests (SEP-1686) | ✅ |

Enable features in `Cargo.toml`:
```toml
[dependencies]
turbomcp-client = { version = "3.5.0", features = ["tcp", "websocket"] }
```

## Architecture

```
┌─────────────────────────────────────────────┐
│            Application Code                 │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│           Client API (Clone-able)           │
│  ├── initialize(), list_tools(), etc.      │
│  └── Handler Registry (elicitation, etc.)  │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│       Protocol Layer (JSON-RPC)             │
│  ├── Request/Response correlation          │
│  ├── Bidirectional message routing         │
│  └── Capability negotiation                │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│       Transport Layer                       │
│  ├── STDIO, HTTP, TCP, Unix, WebSocket     │
│  ├── TurboTransport (retry, circuit)       │
│  └── Connection management                 │
└─────────────────────────────────────────────┘
```

## Development

### Building

```bash
# Build with default features (STDIO only)
cargo build

# Build with all transport features
cargo build --features tcp,unix,websocket,http

# Build with every feature
cargo build --all-features
```

### Testing

```bash
# Run unit tests
cargo test

# Run with specific features
cargo test --features websocket
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework with server macros
- **[turbomcp-protocol](../turbomcp-protocol/)** - Protocol types and core utilities
- **[turbomcp-transport](../turbomcp-transport/)** - Transport implementations
- **[turbomcp-http](../turbomcp-http/)** - Streamable HTTP client transport and `AuthProvider`

## Resources

- **[MCP Specification](https://modelcontextprotocol.io/)** - Official protocol docs
- **[MCP 2025-11-25 Spec](https://spec.modelcontextprotocol.io/)** - Current supported version
- **[TurboMCP Documentation](https://turbomcp.org)** - Framework docs

## Roadmap

Candidate future work (not on any committed timeline):

- [ ] **Connection Pool Management** — multi-server connection pooling
- [ ] **Session Persistence** — automatic state preservation across reconnects
- [ ] **Batch Requests** — send multiple requests in a single message

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) Rust SDK for the Model Context Protocol.*
