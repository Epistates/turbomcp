# TurboMCP Transport

[![Crates.io](https://img.shields.io/crates/v/turbomcp-transport.svg)](https://crates.io/crates/turbomcp-transport)
[![Documentation](https://docs.rs/turbomcp-transport/badge.svg)](https://docs.rs/turbomcp-transport)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Transport layer implementation for the Model Context Protocol (MCP) with support for multiple transport protocols and connection patterns.

## Overview

`turbomcp-transport` provides transport layer implementations for the Model Context Protocol. It supports multiple transport protocols including STDIO, HTTP/SSE, WebSocket, TCP, and Unix sockets with features for security, reliability, and concurrent usage.

## Key Features

### Multi-Protocol Support
- STDIO - Standard input/output for command-line MCP integration
- HTTP/SSE - HTTP with Server-Sent Events for streaming communication
- WebSocket - Real-time bidirectional communication
- TCP - Direct TCP connections with connection pooling
- Unix Sockets - Inter-process communication on Unix systems

### Security Features
- TLS 1.3 Support - Modern encryption with `rustls`
- CORS Protection - Cross-origin resource sharing configuration
- Security Headers - CSP, HSTS, X-Frame-Options, and more
- Rate Limiting - Token bucket algorithm for request rate control
- Authentication - JWT validation and API key support

### Reliability Features
- Circuit Breaker Pattern - Prevents cascade failures with automatic recovery
- Exponential Backoff - Retry logic with jitter
- Connection Health Monitoring - Automatic detection of connection issues
- Graceful Degradation - Fallback mechanisms and error recovery
- Resource Management - Memory usage controls with cleanup tasks

### Compression Support
- Multiple Algorithms - gzip, brotli, lz4 compression options
- Adaptive Compression - Algorithm selection based on content
- Streaming Support - Low-memory compression for large messages
- Compression Metrics - Performance monitoring capabilities

### SharedTransport for Async Concurrency
- Thread-safe transport sharing - Share transports across multiple async tasks
- Clean API surface - Hide Arc/Mutex complexity from public interfaces
- Protocol compliant - Preserves all transport semantics
- Clone support - Easy sharing with simple `.clone()` operations

## Architecture

```
┌─────────────────────────────────────────────┐
│            TurboMCP Transport               │
├─────────────────────────────────────────────┤
│ Protocol Implementations                   │
│ ├── STDIO (process pipes)                  │
│ ├── HTTP/SSE (web servers)                 │
│ ├── WebSocket (realtime)                   │
│ ├── TCP (network sockets)                  │
│ └── Unix Sockets (IPC)                     │
├─────────────────────────────────────────────┤
│ Security & Authentication                  │
│ ├── TLS 1.3 encryption                    │
│ ├── JWT token validation                   │
│ ├── CORS and security headers             │
│ ├── Rate limiting                          │
│ └── Certificate management                 │
├─────────────────────────────────────────────┤
│ Reliability & Fault Tolerance             │
│ ├── Circuit breaker pattern               │
│ ├── Exponential backoff retry             │
│ ├── Connection pooling                     │
│ ├── Health monitoring                      │
│ └── Graceful degradation                   │
├─────────────────────────────────────────────┤
│ Performance & Optimization                 │
│ ├── Advanced compression                   │
│ ├── Connection reuse                       │
│ ├── Message batching                       │
│ └── Memory-efficient streaming             │
└─────────────────────────────────────────────┘
```

## Transport Protocols

### STDIO Transport

For local process communication:

```rust
use turbomcp_transport::stdio::{StdioTransport, ChildProcessConfig};

// Direct process communication
let transport = StdioTransport::new();

// Child process management
let config = ChildProcessConfig::new()
    .command("/usr/bin/python3")
    .args(["-m", "my_mcp_server"])
    .working_directory("/path/to/server")
    .environment_vars([("DEBUG", "true")]);

let child_transport = StdioTransport::with_child_process(config).await?;
```

### MCP 2025-06-18 Streamable HTTP (Client)

For connecting to HTTP-based MCP servers with full SSE support:

```rust
use turbomcp_transport::streamable_http_client::{
    StreamableHttpClientConfig, StreamableHttpClientTransport
};
use std::time::Duration;

// MCP 2025-06-18 compliant HTTP client with SSE
let config = StreamableHttpClientConfig {
    base_url: "http://localhost:8080".to_string(),
    endpoint_path: "/mcp".to_string(),
    timeout: Duration::from_secs(30),
    ..Default::default()
};

let mut transport = StreamableHttpClientTransport::new(config);
```

### WebSocket Transport

For real-time communication:

```rust
use turbomcp_transport::websocket::WebSocketTransport;

// Connect to WebSocket server
let transport = WebSocketTransport::new("wss://api.example.com/mcp").await?;

// Transport capabilities are predefined:
// - Max message size: 16MB
// - Compression: supported
// - Streaming: supported
// - Bidirectional: supported
```

### TCP Transport

For network socket communication:

```rust
use turbomcp_transport::tcp::TcpTransportBuilder;
use std::net::SocketAddr;

let bind_addr: SocketAddr = "127.0.0.1:8080".parse()?;

let transport = TcpTransportBuilder::new()
    .bind_addr(bind_addr)
    .keep_alive(true)
    .buffer_size(64 * 1024) // 64KB
    .build();
```

### Unix Socket Transport

For local inter-process communication:

```rust
use turbomcp_transport::unix::{UnixTransport, UnixConfig};
use std::path::PathBuf;

let config = UnixConfig {
    socket_path: PathBuf::from("/tmp/mcp.sock"),
    permissions: Some(0o660),
    buffer_size: 8192,
    cleanup_on_disconnect: true,
};

let transport = UnixTransport::new(config);
```

## Security Configuration

### Security Validator Setup

```rust
use turbomcp_transport::{SecurityConfigBuilder, AuthMethod, RateLimitConfig};
use std::time::Duration;

// Build a security validator with authentication and rate limiting
let validator = SecurityConfigBuilder::new()
    .add_allowed_origin("https://app.example.com".to_string())
    .allow_localhost(true)
    .require_authentication(true)
    .with_auth_method(AuthMethod::ApiKey)
    .add_api_key("your-api-key-here".to_string())
    .with_rate_limit(100, Duration::from_secs(60)) // 100 requests per minute
    .build();
```

### Enhanced Security with Session Management

```rust
use turbomcp_transport::EnhancedSecurityConfigBuilder;

// Build enhanced security with session tracking
let enhanced_security = EnhancedSecurityConfigBuilder::new()
    .add_allowed_origin("https://app.example.com".to_string())
    .require_authentication(true)
    .add_api_key("your-api-key".to_string())
    .with_rate_limit(120, Duration::from_secs(60))
    .with_session_timeout(Duration::from_secs(3600)) // 1 hour
    .with_max_sessions_per_client(5)
    .build();

## Reliability Features

### Circuit Breaker Configuration

```rust
use turbomcp_transport::resilience::circuit_breaker::CircuitBreakerConfig;
use std::time::Duration;

// Configure circuit breaker for fault tolerance
let circuit_config = CircuitBreakerConfig {
    failure_threshold: 5,           // Open circuit after 5 failures
    success_threshold: 3,            // Close circuit after 3 successes
    timeout: Duration::from_secs(60), // Wait 60s before trying again
    rolling_window_size: 100,        // Track last 100 operations
    minimum_requests: 10,            // Need 10 requests before opening
};
```

### Retry Configuration

```rust
use turbomcp_transport::resilience::retry::RetryConfig;
use std::time::Duration;

// Configure retry behavior with exponential backoff
let retry_config = RetryConfig {
    max_attempts: 3,
    base_delay: Duration::from_millis(100),
    max_delay: Duration::from_secs(10),
    backoff_multiplier: 2.0,
    jitter_factor: 0.1,
    retry_on_connection_error: true,
    retry_on_timeout: true,
    custom_retry_condition: None,
};
```

## Compression Support

### Message Compression

```rust
use turbomcp_transport::compression::{MessageCompressor, CompressionType};
use serde_json::json;

// Create a compressor with LZ4 compression
let compressor = MessageCompressor::new(CompressionType::Lz4);

// Compress a message
let message = json!({"tool": "add", "args": {"a": 5, "b": 3}});
let compressed = compressor.compress(&message)?;

// Decompress back to JSON
let decompressed = compressor.decompress(&compressed)?;
assert_eq!(message, decompressed);

// Available compression types:
// - CompressionType::None
// - CompressionType::Gzip (with "flate2" feature)
// - CompressionType::Brotli (with "brotli" feature)
// - CompressionType::Lz4 (with "lz4_flex" feature)
```

## Health Monitoring

### Health Check Configuration

```rust
use turbomcp_transport::resilience::health::{HealthCheckConfig, HealthStatus};
use std::time::Duration;

// Configure health checking
let health_config = HealthCheckConfig {
    interval: Duration::from_secs(30),
    timeout: Duration::from_secs(5),
    failure_threshold: 3,      // Unhealthy after 3 failures
    success_threshold: 2,      // Healthy after 2 successes
    custom_check: None,
};

// Health status can be:
// - HealthStatus::Healthy
// - HealthStatus::Unhealthy
// - HealthStatus::Unknown
// - HealthStatus::Checking
```

## Integration Examples

### With TurboMCP Framework

Transport selection is automatic when using the main framework:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MyServer::new();
    
    // Transport selected based on environment/configuration
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => server.run_http("127.0.0.1:8080").await?,
        Ok("websocket") => server.run_websocket("127.0.0.1:8080").await?,
        Ok("tcp") => server.run_tcp("127.0.0.1:8080").await?,
        Ok("unix") => server.run_unix("/tmp/mcp.sock").await?,
        _ => server.run_stdio().await?, // Default
    }
    
    Ok(())
}
```

### Custom Transport Implementation

```rust
use turbomcp_transport::{Transport, TransportMessage, TransportConfig};
use async_trait::async_trait;

struct CustomTransport {
    config: TransportConfig,
    // ... custom fields
}

#[async_trait]
impl Transport for CustomTransport {
    async fn send(&self, message: TransportMessage) -> Result<(), TransportError> {
        // Custom send implementation
        Ok(())
    }
    
    async fn receive(&self) -> Result<TransportMessage, TransportError> {
        // Custom receive implementation
        todo!()
    }
    
    async fn close(&self) -> Result<(), TransportError> {
        // Cleanup implementation
        Ok(())
    }
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
| `compression` | Enable compression algorithms | ✅ |
| `metrics` | Enable metrics collection | ✅ |
| `circuit-breaker` | Enable circuit breaker pattern | ✅ |

## SharedTransport for Async Concurrency

TurboMCP introduces SharedTransport - a thread-safe wrapper that eliminates Arc/Mutex complexity while preserving full transport functionality:

### Basic SharedTransport Usage

```rust
use turbomcp_transport::{StdioTransport, SharedTransport};

// Create and wrap any transport for sharing
let transport = StdioTransport::new();
let shared = SharedTransport::new(transport);

// Connect once
shared.connect().await?;

// Clone for concurrent usage across tasks
let shared1 = shared.clone();
let shared2 = shared.clone();

// Both tasks can use the transport concurrently
let handle1 = tokio::spawn(async move {
    shared1.send(message1).await
});

let handle2 = tokio::spawn(async move {
    shared2.receive().await
});

let (send_result, message) = tokio::join!(handle1, handle2);
```

### Advanced Concurrent Patterns

```rust
use turbomcp_transport::SharedTransport;
use std::sync::Arc;
use tokio::sync::Semaphore;

// Rate-limited concurrent transport operations
let shared_transport = SharedTransport::new(transport);
let semaphore = Arc::new(Semaphore::new(10)); // Max 10 concurrent operations

let tasks = (0..50).map(|i| {
    let transport = shared_transport.clone();
    let semaphore = semaphore.clone();

    tokio::spawn(async move {
        let _permit = semaphore.acquire().await.unwrap();

        let message = create_message(i);
        transport.send(message).await?;
        transport.receive().await
    })
}).collect::<Vec<_>>();

// Wait for all operations to complete
let results = futures::future::join_all(tasks).await;
```

### Integration with Multiple Clients

```rust
use turbomcp_transport::SharedTransport;
use turbomcp_client::Client;

// Share a single transport across multiple clients
let transport = TcpTransport::connect("127.0.0.1:8080").await?;
let shared_transport = SharedTransport::new(transport);

// Create multiple clients sharing the same transport
let client1 = Client::new(shared_transport.clone());
let client2 = Client::new(shared_transport.clone());
let client3 = Client::new(shared_transport.clone());

// Initialize all clients concurrently
let (result1, result2, result3) = tokio::try_join!(
    client1.initialize(),
    client2.initialize(),
    client3.initialize()
)?;

// All clients can now operate independently
tokio::spawn(async move {
    loop {
        let tools = client1.list_tools().await?;
        // Process tools...
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
});

tokio::spawn(async move {
    loop {
        let resources = client2.list_resources().await?;
        // Process resources...
        tokio::time::sleep(Duration::from_secs(45)).await;
    }
});
```

### Benefits

- Clean APIs: No exposed Arc/Mutex types in transport interfaces
- Easy Sharing: Simple `.clone()` for concurrent access
- Thread Safety: Built-in synchronization for async operations
- Zero Overhead: Same performance as direct transport usage
- Protocol Compliant: Preserves all transport semantics exactly
- Universal Compatibility: Works with all transport types (STDIO, HTTP, WebSocket, TCP, Unix)

## Performance Characteristics

### Benchmarks

| Transport | Latency (avg) | Throughput | Memory Usage |
|-----------|---------------|------------|--------------|
| STDIO | 0.1ms | 50k msg/s | 2MB |
| Unix Socket | 0.2ms | 45k msg/s | 3MB |
| TCP | 0.5ms | 30k msg/s | 5MB |
| WebSocket | 1ms | 25k msg/s | 8MB |
| HTTP/SSE | 2ms | 15k msg/s | 10MB |

### Optimization Features

- Connection Pooling - Reuse connections for better performance
- Message Batching - Combine small messages for efficiency
- Smart Compression - Adaptive compression based on content
- Zero-Copy - Minimize memory allocations where possible

## Development

### Building

```bash
# Build with all features
cargo build --all-features

# Build specific transport
cargo build --features http,websocket

# Build without TLS (for testing)
cargo build --no-default-features --features stdio,tcp
```

### Testing

```bash
# Run transport tests
cargo test

# Test with TLS
cargo test --features tls

# Run integration tests
cargo test --test integration

# Test circuit breaker functionality
cargo test circuit_breaker
```

## Security Documentation

For comprehensive security information, see:
- **[Security Features Guide](./SECURITY_FEATURES.md)** - Detailed security documentation
- **[TLS Configuration](./docs/tls.md)** - TLS setup and certificate management
- **[Authentication Guide](./docs/auth.md)** - JWT and API key authentication

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework (uses this crate)
- **[turbomcp-protocol](../turbomcp-protocol/)** - Protocol implementation and core utilities
- **[turbomcp-server](../turbomcp-server/)** - Server framework

## External Resources

- **[Axum Framework](https://github.com/tokio-rs/axum)** - HTTP framework used for HTTP transport
- **[tokio-tungstenite](https://github.com/snapview/tokio-tungstenite)** - WebSocket implementation
- **[rustls](https://github.com/rustls/rustls)** - TLS implementation

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*