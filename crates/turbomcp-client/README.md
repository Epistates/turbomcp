# TurboMCP Client

[![Crates.io](https://img.shields.io/crates/v/turbomcp-client.svg)](https://crates.io/crates/turbomcp-client)
[![Documentation](https://docs.rs/turbomcp-client/badge.svg)](https://docs.rs/turbomcp-client)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Production-ready MCP client with complete MCP 2025-06-18 specification support and plugin middleware system.

## Overview

`turbomcp-client` provides a comprehensive MCP client implementation with:
- ✅ **Full MCP 2025-06-18 compliance** - All server and client features
- ✅ **Bidirectional communication** - Server-initiated requests (sampling, elicitation)
- ✅ **Plugin middleware** - Extensible request/response processing
- ✅ **Sampling protocol support** - Handle server-initiated sampling requests
- ✅ **Transport agnostic** - Works with STDIO, TCP, Unix, WebSocket transports
- ✅ **Thread-safe sharing** - Client is cheaply cloneable via Arc for concurrent async tasks

## Supported Transports

| Transport | Status | Feature Flag | Use Case |
|-----------|--------|--------------|----------|
| **STDIO** | ✅ Full | default | Local process communication |
| **HTTP/SSE** | ✅ Full | `http` | Web-compatible HTTP servers (New in 2.0!) |
| **TCP** | ✅ Full | `tcp` | Network socket communication |
| **Unix** | ✅ Full | `unix` | Fast local IPC |
| **WebSocket** | ✅ Full | `websocket` | Real-time bidirectional |

> **New in 2.0**: HTTP/SSE client transport with beautiful `Client::connect_http()` convenience API!

## Quick Start

### Basic Client (STDIO)

```rust
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> turbomcp_protocol::Result<()> {
    // Create client with STDIO transport
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Initialize connection
    let result = client.initialize().await?;
    println!("Connected to: {}", result.server_info.name);

    // List and call tools
    let tools = client.list_tools().await?;
    for tool in &tools {
        println!("Tool: {} - {}", tool.name,
            tool.description.as_deref().unwrap_or("No description"));
    }

    // Call a tool
    let result = client.call_tool("calculator", Some(
        std::collections::HashMap::from([
            ("operation".to_string(), serde_json::json!("add")),
            ("a".to_string(), serde_json::json!(5)),
            ("b".to_string(), serde_json::json!(3)),
        ])
    )).await?;

    println!("Result: {}", result);
    Ok(())
}
```

### HTTP Client (Beautiful One-Liner)

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_protocol::Result<()> {
    // Beautiful one-liner - connects and initializes automatically!
    let client = Client::connect_http("http://localhost:8080").await?;

    // Ready to use immediately
    let tools = client.list_tools().await?;
    println!("Found {} tools", tools.len());

    Ok(())
}
```

### TCP/Unix Clients (Equally Beautiful)

```rust
// TCP
let client = Client::connect_tcp("127.0.0.1:8765").await?;

// Unix socket
let client = Client::connect_unix("/tmp/mcp.sock").await?;
```

### With ClientBuilder

```rust
use turbomcp_client::ClientBuilder;
use turbomcp_transport::stdio::StdioTransport;

let client = ClientBuilder::new()
    .with_tools(true)
    .with_prompts(true)
    .with_resources(true)
    .with_sampling(false)
    .build(StdioTransport::new())
    .await?;
```

### Cloning Client for Concurrent Usage

```rust
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

// Create client (cheaply cloneable via Arc)
let client = Client::new(StdioTransport::new());

// Initialize once
client.initialize().await?;

// Clone for multiple async tasks - this is cheap (just Arc clone)
let client1 = client.clone();
let client2 = client.clone();

let handle1 = tokio::spawn(async move {
    client1.list_tools().await
});

let handle2 = tokio::spawn(async move {
    client2.list_prompts().await
});

let (tools, prompts) = tokio::try_join!(handle1, handle2)?;
```

## Transport Configuration

### STDIO Transport (Default)

```rust
use turbomcp_transport::stdio::StdioTransport;

// Direct STDIO
let transport = StdioTransport::new();
let mut client = Client::new(transport);
```

### HTTP Transport (New in 2.0!)

```rust
use turbomcp_client::Client;

// Beautiful one-liner - connects and initializes automatically
let client = Client::connect_http("http://localhost:8080").await?;
```

Or with custom configuration:

```rust
use turbomcp_client::Client;
use std::time::Duration;

let client = Client::connect_http_with("http://localhost:8080", |config| {
    config.timeout = Duration::from_secs(60);
    config.endpoint_path = "/api/mcp".to_string();
}).await?;
```

### TCP Transport

```rust
use turbomcp_client::Client;

// Beautiful one-liner - connects and initializes automatically
let client = Client::connect_tcp("127.0.0.1:8765").await?;
```

Or using transport directly:

```rust
use turbomcp_transport::tcp::TcpTransport;
use std::net::SocketAddr;

let server_addr: SocketAddr = "127.0.0.1:8765".parse()?;
let bind_addr: SocketAddr = "0.0.0.0:0".parse()?;  // Any available port
let transport = TcpTransport::new_client(bind_addr, server_addr);
let mut client = Client::new(transport);
client.initialize().await?;
```

### Unix Socket Transport

```rust
use turbomcp_client::Client;

// Beautiful one-liner - connects and initializes automatically
let client = Client::connect_unix("/tmp/mcp.sock").await?;
```

Or using transport directly:

```rust
use turbomcp_transport::unix::UnixTransport;
use std::path::PathBuf;

let transport = UnixTransport::new_client(PathBuf::from("/tmp/mcp.sock"));
let mut client = Client::new(transport);
client.initialize().await?;
```

### WebSocket Transport

```rust
use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalTransport,
    WebSocketBidirectionalConfig,
};

let config = WebSocketBidirectionalConfig {
    url: Some("ws://localhost:8080".to_string()),
    ..Default::default()
};

let transport = WebSocketBidirectionalTransport::new(config).await?;
let mut client = Client::new(transport);
```

## Advanced Features

### Robust Transport with Retry & Circuit Breaker

```rust
use turbomcp_client::ClientBuilder;
use turbomcp_transport::stdio::StdioTransport;

// Use high-reliability preset
let client = ClientBuilder::new()
    .with_high_reliability()  // Configures retry, circuit breaker, health checks
    .build_robust(StdioTransport::new())
    .await?;
```

### Custom Robustness Configuration

```rust
use turbomcp_transport::resilience::{
    retry::RetryConfig,
    circuit_breaker::CircuitBreakerConfig,
    health::HealthCheckConfig,
};
use std::time::Duration;

let client = ClientBuilder::new()
    .with_retry_config(RetryConfig {
        max_attempts: 5,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(30),
        backoff_multiplier: 2.0,
        jitter_factor: 0.1,
        retry_on_connection_error: true,
        retry_on_timeout: true,
        custom_retry_condition: None,
    })
    .with_circuit_breaker_config(CircuitBreakerConfig {
        failure_threshold: 5,
        success_threshold: 2,
        timeout: Duration::from_secs(60),
        rolling_window_size: 100,
        minimum_requests: 10,
    })
    .with_health_check_config(HealthCheckConfig {
        interval: Duration::from_secs(30),
        timeout: Duration::from_secs(5),
        failure_threshold: 3,
        success_threshold: 1,
        custom_check: None,
    })
    .build_robust(StdioTransport::new())
    .await?;
```

### Plugin Middleware

```rust
use turbomcp_client::ClientBuilder;
use turbomcp_client::plugins::{MetricsPlugin, PluginConfig};
use std::sync::Arc;

let client = ClientBuilder::new()
    .with_plugin(Arc::new(MetricsPlugin::new(PluginConfig::Metrics)))
    .build(StdioTransport::new())
    .await?;
```

### Sampling Handler Integration

Handle server-initiated sampling requests by implementing a custom sampling handler:

```rust
use turbomcp_client::sampling::SamplingHandler;
use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult, Role, Content, TextContent};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug)]
struct MySamplingHandler {
    // Your LLM integration (OpenAI, Anthropic, local model, etc.)
}

#[async_trait]
impl SamplingHandler for MySamplingHandler {
    async fn handle_create_message(&self, request: CreateMessageRequest)
        -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>>
    {
        // Forward to your LLM service
        // Return the generated response
        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: "Generated response".to_string(),
                annotations: None,
                meta: None,
            }),
            model: Some("your-model".to_string()),
            stop_reason: None,
        })
    }
}

// Register the handler
let handler = Arc::new(MySamplingHandler { /* ... */ });
client.set_sampling_handler(handler);
```

**Note:** TurboMCP provides the sampling protocol infrastructure. You implement your own LLM integration (OpenAI SDK, Anthropic SDK, local models, etc.) as needed for your use case.

### Handler Registration

```rust
use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug)]
struct MyElicitationHandler;

#[async_trait]
impl ElicitationHandler for MyElicitationHandler {
    async fn handle_elicitation(&self, request: ElicitationRequest)
        -> Result<ElicitationResponse, Box<dyn std::error::Error + Send + Sync>>
    {
        // Prompt user for input based on request.schema
        let user_input = collect_user_input(request.schema)?;
        Ok(ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(user_input),
        })
    }
}

let client = ClientBuilder::new()
    .with_elicitation_handler(Arc::new(MyElicitationHandler))
    .build(StdioTransport::new())
    .await?;
```

## MCP Operations

### Tools

```rust
// List available tools
let tools = client.list_tools().await?;
for tool in &tools {
    println!("{}: {}", tool.name, tool.description.as_deref().unwrap_or(""));
}

// List tool names only
let names = client.list_tool_names().await?;

// Call a tool
use std::collections::HashMap;
let mut args = HashMap::new();
args.insert("text".to_string(), serde_json::json!("Hello, world!"));
let result = client.call_tool("echo", Some(args)).await?;
```

### Prompts

```rust
use turbomcp_protocol::types::PromptInput;

// List prompts
let prompts = client.list_prompts().await?;

// Get prompt with arguments
let prompt_args = PromptInput {
    arguments: Some(std::collections::HashMap::from([
        ("language".to_string(), "rust".to_string()),
        ("topic".to_string(), "async programming".to_string()),
    ])),
};

let result = client.get_prompt("code_review", Some(prompt_args)).await?;
println!("Prompt: {}", result.description.unwrap_or_default());
for message in result.messages {
    println!("{:?}: {}", message.role, message.content);
}
```

### Resources

```rust
// List resources
let resources = client.list_resources().await?;

// Read a resource
let content = client.read_resource("file:///etc/hosts").await?;

// List resource templates
let templates = client.list_resource_templates().await?;
```

### Completions

```rust
use turbomcp_protocol::types::CompletionContext;

// Complete a prompt argument
let completions = client.complete_prompt(
    "code_review",
    "framework",
    "tok",  // Partial input
    None
).await?;

for value in completions.completion.values {
    println!("Suggestion: {}", value);
}

// Complete with context
let mut context_args = std::collections::HashMap::new();
context_args.insert("language".to_string(), "rust".to_string());
let context = CompletionContext { arguments: Some(context_args) };

let completions = client.complete_prompt(
    "code_review",
    "framework",
    "tok",
    Some(context)
).await?;
```

### Subscriptions

```rust
use turbomcp_protocol::types::LogLevel;

// Subscribe to resource updates
client.subscribe("file:///config.json").await?;

// Set logging level
client.set_log_level(LogLevel::Debug).await?;

// Unsubscribe
client.unsubscribe("file:///config.json").await?;
```

### Health Monitoring

```rust
// Send ping to check connection
let ping_result = client.ping().await?;
println!("Server responded: {:?}", ping_result);
```

## Bidirectional Communication

### Processing Server-Initiated Requests

```rust
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

// Create and initialize client
let client = Client::new(StdioTransport::new());
client.initialize().await?;

// Message processing is automatic! The MessageDispatcher runs in the background.
// No need for manual message loops - just use the client directly.

// Perform operations - bidirectional communication works automatically
let tools = client.list_tools().await?;
```

## Error Handling

```rust
use turbomcp_protocol::Error;

match client.call_tool("my_tool", None).await {
    Ok(result) => println!("Success: {}", result),
    Err(Error::Transport(msg)) => eprintln!("Transport error: {}", msg),
    Err(Error::Protocol(msg)) => eprintln!("Protocol error: {}", msg),
    Err(Error::BadRequest(msg)) => eprintln!("Bad request: {}", msg),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Examples

See the `examples/` directory for complete working examples:

- **`sampling_client.rs`** - Client with server-initiated sampling protocol
- **`elicitation_interactive_client.rs`** - Interactive elicitation handling

Run examples:
```bash
cargo run --example sampling_client --features websocket
cargo run --example elicitation_interactive_client
```

## Feature Flags

| Feature | Description | Status |
|---------|-------------|--------|
| `default` | STDIO transport only | ✅ |
| `tcp` | TCP transport | ✅ |
| `unix` | Unix socket transport | ✅ |
| `websocket` | WebSocket transport | ✅ |
| `http` | HTTP/SSE (server-side only) | ⚠️ |

Enable features in `Cargo.toml`:
```toml
[dependencies]
turbomcp-client = { version = "2.0.0", features = ["tcp", "websocket"] }
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
│  ├── Handler Registry (elicitation, etc.)  │
│  └── Plugin Registry (metrics, etc.)       │
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
│  ├── STDIO, TCP, Unix, WebSocket           │
│  ├── RobustTransport (retry, circuit)      │
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

# Build with robustness features
cargo build --all-features
```

### Testing

```bash
# Run unit tests
cargo test

# Run with specific features
cargo test --features websocket

# Run examples
cargo run --example sampling_client
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework with server macros
- **[turbomcp-protocol](../turbomcp-protocol/)** - Protocol types and core utilities
- **[turbomcp-transport](../turbomcp-transport/)** - Transport implementations

## Resources

- **[MCP Specification](https://modelcontextprotocol.io/)** - Official protocol docs
- **[MCP 2025-06-18 Spec](https://spec.modelcontextprotocol.io/2025-06-18/)** - Current version
- **[TurboMCP Documentation](https://turbomcp.org)** - Framework docs

## Roadmap

### Completed in 2.0

- [x] **HTTP/SSE Client Transport** - Client-side HTTP/SSE with `Client::connect_http()`
- [x] **Convenience Constructors** - One-liner client creation for all transports
- [x] **Ergonomic Config Builders** - Simplified configuration APIs

### Planned Features

- [ ] **Connection Pool Management** - Multi-server connection pooling
- [ ] **Session Persistence** - Automatic state preservation across reconnects
- [ ] **Roots Handler** - Complete filesystem roots implementation
- [ ] **Progress Reporting** - Client-side progress emission
- [ ] **Batch Requests** - Send multiple requests in single message

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*
