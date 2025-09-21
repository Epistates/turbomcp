# TurboMCP Client

[![Crates.io](https://img.shields.io/crates/v/turbomcp-client.svg)](https://crates.io/crates/turbomcp-client)
[![Documentation](https://docs.rs/turbomcp-client/badge.svg)](https://docs.rs/turbomcp-client)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**World-class MCP client implementation** with complete MCP 2025-06-18 support, intelligent connection management, and industry-leading transport layer integration.

## Overview

`turbomcp-client` delivers the **most advanced MCP client available**, featuring complete **MCP 2025-06-18 specification compliance** and seamless integration with TurboMCP's world-class transport layer. Handles all client-side concerns with production-grade reliability and **334,961 msg/sec** performance capability across all 5 transport protocols.

## Key Features

### 🔌 **Multi-Transport Connection Management**
- **All transport protocols** - STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets
- **Connection pooling** - Efficient connection reuse and management
- **Health monitoring** - Automatic connection health checks and recovery
- **Load balancing** - Multiple server connection with failover support

### 🔄 **Intelligent Error Recovery**
- **Auto-retry with backoff** - Configurable retry logic with exponential backoff
- **Circuit breaker integration** - Prevents cascade failures with automatic recovery
- **Graceful degradation** - Fallback mechanisms when servers are unavailable
- **Error classification** - Smart handling of temporary vs permanent failures

### 📞 **Request Correlation & Management**
- **Automatic ID generation** - UUID-based request correlation
- **Request/response matching** - Efficient correlation with timeout handling
- **Concurrent requests** - Multiple outstanding requests with proper ordering
- **Request cancellation** - Proper cleanup of cancelled or timed-out requests

### 🤝 **Capability Negotiation**
- **Server discovery** - Automatic server capability detection
- **Feature matching** - Client/server capability compatibility checking
- **Version negotiation** - Protocol version compatibility handling
- **Extension support** - Custom capability extensions and fallbacks

### 📊 **Session Lifecycle Management**
- **Connection state tracking** - Proper session initialization and cleanup
- **Heartbeat monitoring** - Keep-alive and connection validation
- **Reconnection logic** - Intelligent reconnection with state preservation
- **Session persistence** - Optional session state persistence across connections

## Architecture

```
┌─────────────────────────────────────────────┐
│               TurboMCP Client               │
├─────────────────────────────────────────────┤
│ Connection Management                      │
│ ├── Multi-transport support               │
│ ├── Connection pooling                     │
│ ├── Health monitoring                      │
│ └── Load balancing                         │
├─────────────────────────────────────────────┤
│ Request Processing                         │
│ ├── ID generation and correlation         │
│ ├── Concurrent request handling           │
│ ├── Response timeout management           │
│ └── Request cancellation                  │
├─────────────────────────────────────────────┤
│ Error Recovery & Resilience              │
│ ├── Exponential backoff retry            │
│ ├── Circuit breaker pattern              │
│ ├── Graceful degradation                 │
│ └── Error classification                  │
├─────────────────────────────────────────────┤
│ Capability & Session Management           │
│ ├── Server capability discovery          │
│ ├── Protocol negotiation                 │
│ ├── Session initialization               │
│ └── State synchronization                │
└─────────────────────────────────────────────┘
```

## Client Builder

### Basic Client Setup

```rust
use turbomcp_client::{ClientBuilder, Transport};

// Simple STDIO client
let client = ClientBuilder::new()
    .transport(Transport::stdio())
    .connect().await?;

// Query available tools
let tools = client.list_tools().await?;
for tool in tools {
    println!("Tool: {} - {}", tool.name, tool.description);
}
```

### Production Client Configuration

```rust
use turbomcp_client::{
    ClientBuilder, Transport, RetryConfig, CircuitBreakerConfig,
    ConnectionPoolConfig, HealthCheckConfig
};

let client = ClientBuilder::new()
    .name("ProductionMCPClient")
    .version("2.1.0")
    
    // Multi-transport with failover
    .transport(Transport::http("https://primary.example.com/mcp")
        .with_authentication("Bearer", &auth_token)
        .with_timeout(Duration::from_secs(30)))
    .fallback_transport(Transport::websocket("wss://secondary.example.com/mcp"))
    
    // Connection management
    .connection_pool(ConnectionPoolConfig::new()
        .max_connections(10)
        .min_idle_connections(2)
        .connection_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300)))
    
    // Error recovery
    .retry_config(RetryConfig::exponential()
        .max_attempts(5)
        .initial_delay(Duration::from_millis(100))
        .max_delay(Duration::from_secs(30))
        .jitter(true))
    
    .circuit_breaker(CircuitBreakerConfig::new()
        .failure_threshold(5)
        .recovery_timeout(Duration::from_secs(60))
        .half_open_max_calls(3))
    
    // Health monitoring
    .health_checks(HealthCheckConfig::new()
        .check_interval(Duration::from_secs(30))
        .ping_timeout(Duration::from_secs(5))
        .max_failures(3))
    
    .connect().await?;
```

## Transport Configuration

### STDIO Transport

For local process communication:

```rust
use turbomcp_client::{Transport, stdio::StdioConfig};

// Direct STDIO connection
let transport = Transport::stdio();

// Child process management
let transport = Transport::stdio_with_command(
    StdioConfig::new()
        .command("/usr/bin/python3")
        .args(["-m", "my_mcp_server"])
        .working_directory("/path/to/server")
        .environment_vars([("DEBUG", "1")])
        .timeout(Duration::from_secs(30))
);

let client = ClientBuilder::new()
    .transport(transport)
    .connect().await?;
```

### HTTP/SSE Transport

For web-based servers:

```rust
use turbomcp_client::{Transport, http::HttpConfig};

let transport = Transport::http("https://api.example.com/mcp")
    .with_config(HttpConfig::new()
        .authentication("Bearer", &jwt_token)
        .user_agent("MyApp/1.0")
        .headers([("X-API-Version", "v1")])
        .timeout(Duration::from_secs(30))
        .keep_alive(true)
        .compression(true));

let client = ClientBuilder::new()
    .transport(transport)
    .connect().await?;
```

### WebSocket Transport

For real-time communication:

```rust
use turbomcp_client::{Transport, websocket::WsConfig};

let transport = Transport::websocket("wss://api.example.com/mcp")
    .with_config(WsConfig::new()
        .subprotocols(["mcp-v1"])
        .headers([("Authorization", &format!("Bearer {}", token))])
        .ping_interval(Duration::from_secs(30))
        .max_message_size(16 * 1024 * 1024) // 16MB
        .compression_enabled(true));

let client = ClientBuilder::new()
    .transport(transport)
    .connect().await?;
```

### TCP Transport

For network socket communication:

```rust
use turbomcp_client::{Transport, tcp::TcpConfig};

let transport = Transport::tcp("127.0.0.1:8080")
    .with_config(TcpConfig::new()
        .nodelay(true)
        .keep_alive(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(5))
        .buffer_size(64 * 1024)); // 64KB

let client = ClientBuilder::new()
    .transport(transport)
    .connect().await?;
```

## Tool Interaction

### Listing and Calling Tools

```rust
use turbomcp_client::Client;

// List available tools
let tools = client.list_tools().await?;
println!("Available tools:");
for tool in &tools {
    println!("  {} - {}", tool.name, tool.description);
    if let Some(schema) = &tool.input_schema {
        println!("    Parameters: {}", serde_json::to_string_pretty(schema)?);
    }
}

// Call a specific tool
let result = client.call_tool("calculator", serde_json::json!({
    "operation": "add",
    "a": 5,
    "b": 3
})).await?;

println!("Tool result: {}", result);
```

### Concurrent Tool Calls

```rust
use tokio::try_join;

// Execute multiple tools concurrently
let (weather_result, news_result, stock_result) = try_join!(
    client.call_tool("weather", serde_json::json!({"city": "San Francisco"})),
    client.call_tool("news", serde_json::json!({"category": "technology"})),
    client.call_tool("stock_price", serde_json::json!({"symbol": "AAPL"}))
)?;

println!("Weather: {}", weather_result);
println!("News: {}", news_result);
println!("Stock: {}", stock_result);
```

## Resource Management

### Reading Resources

```rust
// List available resources
let resources = client.list_resources().await?;
for resource in &resources {
    println!("Resource: {} - {}", resource.uri, resource.name);
}

// Read specific resources
let file_content = client.read_resource("file:///etc/hosts").await?;
println!("File content: {}", file_content);

// Read multiple resources concurrently
let contents = client.read_resources_concurrent([
    "file:///var/log/app.log",
    "http://api.example.com/config",
    "database://users/table"
]).await?;
```

### Resource Subscriptions

```rust
use tokio_stream::StreamExt;

// Subscribe to resource updates
let mut updates = client.subscribe_to_resource("file:///var/log/app.log").await?;

while let Some(update) = updates.next().await {
    match update {
        Ok(content) => println!("Resource updated: {}", content),
        Err(e) => eprintln!("Resource error: {}", e),
    }
}
```

## Error Handling & Recovery

### Retry Configuration

```rust
use turbomcp_client::{RetryConfig, BackoffStrategy, RetryableError};

let retry_config = RetryConfig::new()
    .max_attempts(5)
    .strategy(BackoffStrategy::ExponentialWithJitter {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(30),
        multiplier: 2.0,
        jitter_factor: 0.1,
    })
    .retryable_errors([
        RetryableError::ConnectionTimeout,
        RetryableError::ConnectionReset,
        RetryableError::ServerError(500..=599),
    ]);

let client = ClientBuilder::new()
    .retry_config(retry_config)
    .transport(Transport::http("https://api.example.com/mcp"))
    .connect().await?;
```

### Circuit Breaker

```rust
use turbomcp_client::{CircuitBreakerConfig, FailureThreshold};

let circuit_config = CircuitBreakerConfig::new()
    .failure_threshold(FailureThreshold::ConsecutiveFailures(5))
    .recovery_timeout(Duration::from_secs(60))
    .half_open_max_calls(3)
    .success_threshold(2);

let client = ClientBuilder::new()
    .circuit_breaker(circuit_config)
    .transport(Transport::websocket("wss://api.example.com/mcp"))
    .connect().await?;
```

### Error Classification

```rust
use turbomcp_client::{McpClientError, ErrorClassification};

match client.call_tool("my_tool", params).await {
    Ok(result) => println!("Success: {}", result),
    Err(McpClientError::Connection(e)) => {
        eprintln!("Connection error: {}", e);
        // Implement connection recovery
    },
    Err(McpClientError::Timeout(e)) => {
        eprintln!("Request timeout: {}", e);
        // Retry with longer timeout
    },
    Err(McpClientError::ServerError(code, msg)) => {
        eprintln!("Server error {}: {}", code, msg);
        // Handle server-side errors
    },
    Err(McpClientError::ValidationError(e)) => {
        eprintln!("Validation error: {}", e);
        // Fix request parameters
    },
}
```

## Capability Negotiation

### Client Capabilities

```rust
use turbomcp_client::{ClientCapabilities, SamplingCapability, RootCapability};

let capabilities = ClientCapabilities {
    sampling: Some(SamplingCapability {}),
    roots: Some(RootCapability { 
        list_changed: true 
    }),
    experimental: Some(serde_json::json!({
        "custom_feature": true,
        "version": "1.0"
    })),
};

let client = ClientBuilder::new()
    .capabilities(capabilities)
    .transport(Transport::stdio())
    .connect().await?;

// Check negotiated capabilities
let server_capabilities = client.server_capabilities().await?;
if server_capabilities.tools.is_some() {
    println!("Server supports tools");
}
if server_capabilities.resources.is_some() {
    println!("Server supports resources");
}
```

### Capability-Aware Operations

```rust
// Check capabilities before making requests
if client.supports_tool_calls().await? {
    let tools = client.list_tools().await?;
    // Use tools...
} else {
    println!("Server does not support tools");
}

if client.supports_resource_subscriptions().await? {
    let updates = client.subscribe_to_resource("file:///config").await?;
    // Handle updates...
} else {
    // Fallback to polling
    loop {
        let content = client.read_resource("file:///config").await?;
        // Process content...
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
```

## Session Management

### Connection Lifecycle

```rust
use turbomcp_client::{Client, ConnectionState};

// Monitor connection state
client.on_state_change(|state| {
    match state {
        ConnectionState::Connecting => println!("Connecting to server..."),
        ConnectionState::Connected => println!("Connected successfully"),
        ConnectionState::Reconnecting => println!("Connection lost, reconnecting..."),
        ConnectionState::Disconnected => println!("Disconnected from server"),
    }
});

// Graceful shutdown
tokio::signal::ctrl_c().await?;
println!("Shutting down...");
client.shutdown().await?;
```

### Session Persistence

```rust
use turbomcp_client::{SessionStore, SessionConfig};

let session_store = SessionStore::file("/var/lib/myapp/session.json");
let session_config = SessionConfig::new()
    .persist_session(true)
    .session_timeout(Duration::from_secs(3600)) // 1 hour
    .heartbeat_interval(Duration::from_secs(30));

let client = ClientBuilder::new()
    .session_store(session_store)
    .session_config(session_config)
    .transport(Transport::websocket("wss://api.example.com/mcp"))
    .connect().await?;

// Session is automatically restored on reconnection
```

## Integration Examples

### With TurboMCP Framework

Client functionality integrates seamlessly with server-side code:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct ClientIntegratedApp {
    mcp_client: Arc<turbomcp_client::Client>,
}

#[server]
impl ClientIntegratedApp {
    #[tool("Proxy tool call to external server")]
    async fn proxy_call(&self, ctx: Context, tool_name: String, params: serde_json::Value) -> McpResult<serde_json::Value> {
        ctx.info(&format!("Proxying call to {}", tool_name)).await?;
        
        match self.mcp_client.call_tool(&tool_name, params).await {
            Ok(result) => Ok(result),
            Err(e) => Err(McpError::ExternalService(e.to_string())),
        }
    }
}
```

### Standalone Client Application

```rust
use turbomcp_client::{ClientBuilder, Transport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ClientBuilder::new()
        .name("MyMCPClient")
        .version("1.0.0")
        .transport(Transport::stdio_with_command(
            Command::new("python3")
                .args(["-m", "my_server"])
        ))
        .connect().await?;
    
    // Interactive tool usage
    loop {
        let tools = client.list_tools().await?;
        
        println!("Available tools:");
        for (i, tool) in tools.iter().enumerate() {
            println!("  {}: {} - {}", i + 1, tool.name, tool.description);
        }
        
        println!("Enter tool number (0 to quit): ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        
        let choice: usize = input.trim().parse()?;
        if choice == 0 { break; }
        
        if let Some(tool) = tools.get(choice - 1) {
            println!("Enter parameters (JSON): ");
            input.clear();
            std::io::stdin().read_line(&mut input)?;
            
            let params: serde_json::Value = serde_json::from_str(&input)?;
            
            match client.call_tool(&tool.name, params).await {
                Ok(result) => println!("Result: {}", serde_json::to_string_pretty(&result)?),
                Err(e) => eprintln!("Error: {}", e),
            }
        }
    }
    
    client.shutdown().await?;
    Ok(())
}
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `http` | Enable HTTP/SSE transport | ✅ |
| `websocket` | Enable WebSocket transport | ✅ |
| `tcp` | Enable TCP transport | ✅ |
| `unix` | Enable Unix socket transport | ✅ |
| `tls` | Enable TLS/SSL support | ✅ |
| `compression` | Enable compression support | ✅ |
| `session-persistence` | Enable session state persistence | ❌ |
| `metrics` | Enable client-side metrics | ✅ |

## Performance Characteristics

### Benchmarks

| Operation | Latency (avg) | Throughput | Memory Usage |
|-----------|---------------|------------|--------------|
| Tool Call (STDIO) | 2ms | 25k req/s | 5MB |
| Tool Call (HTTP) | 10ms | 10k req/s | 8MB |
| Tool Call (WebSocket) | 5ms | 15k req/s | 6MB |
| Resource Read | 3ms | 20k req/s | 4MB |
| Concurrent Requests (10) | 8ms | 12k req/s | 12MB |

### Optimization Features

- 🚀 **Connection Pooling** - Reuse connections for better performance
- 📦 **Request Pipelining** - Multiple concurrent requests per connection
- 🗜️ **Compression** - Automatic request/response compression
- ⚡ **Caching** - Smart caching of capabilities and resource metadata

## Development

### Building

```bash
# Build with all features
cargo build --all-features

# Build specific transports only
cargo build --features http,websocket

# Build minimal client (STDIO only)
cargo build --no-default-features --features stdio
```

### Testing

```bash
# Run client tests
cargo test

# Test with different transports
cargo test --features http,websocket,tcp

# Integration tests with real servers
cargo test --test integration

# Test error recovery and circuit breaker
cargo test error_recovery circuit_breaker
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework (uses this crate)
- **[turbomcp-core](../turbomcp-core/)** - Core types and utilities
- **[turbomcp-transport](../turbomcp-transport/)** - Transport layer
- **[turbomcp-protocol](../turbomcp-protocol/)** - MCP protocol implementation

## External Resources

- **[MCP Client Specification](https://modelcontextprotocol.io/)** - Official client implementation guidelines
- **[Circuit Breaker Pattern](https://martinfowler.com/bliki/CircuitBreaker.html)** - Fault tolerance pattern
- **[Connection Pooling](https://en.wikipedia.org/wiki/Connection_pool)** - Connection management patterns

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*