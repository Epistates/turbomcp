# Transports

Configure and use multiple transport protocols for your MCP server. TurboMCP v3 introduces modular transport crates for maximum flexibility.

## Overview

TurboMCP v3 supports multiple transport protocols through individual crates:

| Transport | Crate | Feature | Use Case |
|-----------|-------|---------|----------|
| **STDIO** | `turbomcp-stdio` | `stdio` | CLI, Claude desktop |
| **HTTP** | `turbomcp-http` | `http` | Web applications, REST APIs |
| **WebSocket** | `turbomcp-websocket` | `websocket` | Real-time bidirectional |
| **TCP** | `turbomcp-tcp` | `tcp` | High performance |
| **Unix** | `turbomcp-unix` | `unix` | Local IPC |
| **gRPC** | `turbomcp-grpc` | `grpc` | Enterprise, microservices (v3) |

## Basic Usage

### Single Transport (STDIO)

```rust
#[tokio::main]
async fn main() -> McpResult<()> {
    let server = McpServer::new()
        .stdio()
        .run()
        .await?;

    Ok(())
}
```

### Multiple Transports

```rust
let server = McpServer::new()
    .stdio()                  // Enable STDIO
    .http(8080)               // Enable HTTP on port 8080
    .websocket(8081)          // Enable WebSocket on port 8081
    .tcp(9000)                // Enable TCP on port 9000
    .grpc(50051)              // Enable gRPC on port 50051 (v3)
    .run()
    .await?;
```

## STDIO Transport

Standard input/output for CLI tools and local testing.

**Use cases:**
- Claude desktop integration
- Command-line tools
- Local development
- Testing

**Features:**
- No network configuration needed
- Single client connection
- Blocking I/O on stdin/stdout

```rust
let server = McpServer::new()
    .stdio()
    .run()
    .await?;
```

## HTTP Transport

REST API with Server-Sent Events (SSE) for server-to-client communication.

**Use cases:**
- Web applications
- Mobile clients
- Cross-network communication
- Public APIs

**Features:**
- RESTful endpoint for tool calls
- SSE for server-to-client notifications
- Connection pooling
- CORS support

```rust
let server = McpServer::new()
    .http(8080)  // Listen on port 8080
    .run()
    .await?;
```

**Endpoints:**

| Method | Path | Description |
|--------|------|-------------|
| POST | `/tools/call` | Call a tool |
| GET | `/tools/list` | List available tools |
| POST | `/resources/read` | Read a resource |
| GET | `/resources/list` | List resources |
| GET | `/events` | Server-Sent Events stream |

**Example client:**

```bash
# Call a tool
curl -X POST http://localhost:8080/tools/call \
  -H "Content-Type: application/json" \
  -d '{
    "name": "get_weather",
    "arguments": {"city": "New York"}
  }'
```

## WebSocket Transport

Full-duplex WebSocket for bidirectional real-time communication.

**Use cases:**
- Real-time applications
- Bidirectional elicitation
- High-frequency updates
- Interactive tools

**Features:**
- Full duplex communication
- Low latency
- Automatic reconnection
- Heartbeat/ping-pong

```rust
let server = McpServer::new()
    .websocket(8081)  // Listen on port 8081
    .run()
    .await?;
```

**Connection URL:** `ws://localhost:8081`

**Example client (JavaScript):**

```javascript
const ws = new WebSocket('ws://localhost:8081');

ws.onopen = () => {
    ws.send(JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'tools/call',
        params: {
            name: 'get_weather',
            arguments: { city: 'New York' }
        }
    }));
};

ws.onmessage = (event) => {
    const response = JSON.parse(event.data);
    console.log('Response:', response);
};
```

## TCP Transport

Low-level TCP networking for custom protocols.

**Use cases:**
- Custom binary protocols
- High-performance scenarios
- Private networks
- Legacy system integration

```rust
let server = McpServer::new()
    .tcp(9000)  // Listen on port 9000
    .run()
    .await?;
```

**Protocol:** JSON-RPC 2.0 messages separated by newlines

## Unix Socket Transport

Local inter-process communication.

**Use cases:**
- Local service integration
- Docker containers
- Multi-process applications

```rust
let server = McpServer::new()
    .unix("/tmp/mcp.sock")  // Create socket at path
    .run()
    .await?;
```

## gRPC Transport (v3)

High-performance gRPC transport using tonic.

**Use cases:**
- Enterprise applications
- Microservices
- Load balancing
- Streaming

```rust
use turbomcp_grpc::server::McpGrpcServer;

let server = McpGrpcServer::builder()
    .server_info("my-server", "1.0.0")
    .add_tool(my_tool)
    .build();

tonic::transport::Server::builder()
    .add_service(server.into_service())
    .serve("[::1]:50051".parse()?)
    .await?;
```

See [gRPC API Reference](../api/grpc.md) for full details.

## Wire Codecs (v3)

TurboMCP v3 supports pluggable wire codecs:

```rust
use turbomcp_wire::{Codec, JsonCodec, SimdJsonCodec};

// Standard JSON (default)
let codec = JsonCodec::new();

// SIMD-accelerated JSON (2-4x faster)
let codec = SimdJsonCodec::new();
```

Configure per transport:

```rust
use turbomcp_http::HttpTransportConfig;

let config = HttpTransportConfig::builder()
    .codec("simd")  // Use SIMD codec
    .build();
```

## Configuration

### Port Configuration

```rust
let server = McpServer::new()
    .http(8080)        // HTTP on port 8080
    .websocket(8081)   // WebSocket on port 8081
    .tcp(9000)         // TCP on port 9000
    .grpc(50051)       // gRPC on port 50051
    .run()
    .await?;
```

### TLS/SSL

```rust
let server = McpServer::new()
    .http(8080)
    .with_tls(TlsConfig {
        cert_path: "path/to/cert.pem",
        key_path: "path/to/key.pem",
    })
    .run()
    .await?;
```

### CORS Configuration

```rust
let server = McpServer::new()
    .http(8080)
    .with_cors(CorsConfig {
        allowed_origins: vec!["https://example.com"],
        allowed_methods: vec!["POST", "GET"],
        allowed_headers: vec!["Content-Type"],
        max_age: 3600,
    })
    .run()
    .await?;
```

## Connection Management

### Graceful Shutdown

```rust
let server = McpServer::new()
    .stdio()
    .with_graceful_shutdown(Duration::from_secs(30))
    .run()
    .await?;
```

### Connection Pooling

HTTP and TCP transports automatically manage connection pools:

```rust
let server = McpServer::new()
    .http(8080)
    .with_connection_pool(ConnectionPoolConfig {
        min_connections: 10,
        max_connections: 100,
        timeout: Duration::from_secs(30),
    })
    .run()
    .await?;
```

### Circuit Breaker

Automatic protection against cascading failures:

```rust
let server = McpServer::new()
    .http(8080)
    .with_circuit_breaker(CircuitBreakerConfig {
        failure_threshold: 5,
        success_threshold: 2,
        timeout: Duration::from_secs(60),
    })
    .run()
    .await?;
```

## Transport Selection Guide

| Transport | Latency | Throughput | Duplex | Best For |
|-----------|---------|-----------|--------|----------|
| STDIO | Low | Medium | Half | CLI, local dev |
| HTTP | Medium | High | Half | Web, REST APIs |
| WebSocket | Low | Medium | Full | Real-time, interactive |
| TCP | Low | Very High | Full | High performance |
| Unix Socket | Very Low | Very High | Full | Local IPC |
| gRPC | Low | Very High | Full | Enterprise, microservices |

## Using Individual Transport Crates

For fine-grained control, depend on individual crates:

```toml
[dependencies]
turbomcp-http = "3.0"
turbomcp-websocket = "3.0"
turbomcp-grpc = "3.0"
```

```rust
use turbomcp_http::HttpTransport;
use turbomcp_websocket::WebSocketTransport;

let http = HttpTransport::new(8080);
let ws = WebSocketTransport::new(8081);
```

## Monitoring & Metrics

Get transport statistics:

```rust
let stats = server.transport_stats().await?;

println!("Active connections: {}", stats.active_connections);
println!("Total requests: {}", stats.total_requests);
println!("Error rate: {}%", stats.error_rate);
```

With OpenTelemetry (v3):

```rust
use turbomcp_telemetry::tower::TelemetryLayer;

let service = ServiceBuilder::new()
    .layer(TelemetryLayer::new(config))
    .service(transport);
```

## Troubleshooting

### "Address already in use"

Port is already bound. Use a different port:

```rust
.http(8081)  // Use different port
```

### Connection timeouts

Increase timeout or adjust network:

```rust
.with_connection_timeout(Duration::from_secs(60))
```

### WebSocket connection drops

Client may not support long connections. Implement reconnection:

```javascript
ws.onclose = () => {
    setTimeout(() => {
        ws = new WebSocket('ws://localhost:8081');
    }, 5000);
};
```

### gRPC connection issues

Check TLS configuration and port accessibility:

```rust
// Without TLS (development)
tonic::transport::Server::builder()
    .add_service(server.into_service())
    .serve("[::1]:50051".parse()?)
    .await?;

// With TLS (production)
let identity = Identity::from_pem(cert, key);
tonic::transport::Server::builder()
    .tls_config(ServerTlsConfig::new().identity(identity))?
    .add_service(server.into_service())
    .serve("[::1]:50051".parse()?)
    .await?;
```

## Performance Tuning

### For High Throughput

Use TCP, WebSocket, or gRPC:

```rust
let server = McpServer::new()
    .tcp(9000)
    .websocket(8081)
    .grpc(50051)
    .with_buffer_size(1024 * 1024)  // 1MB buffers
    .run()
    .await?;
```

### For Low Latency

Use WebSocket, Unix Socket, or gRPC:

```rust
let server = McpServer::new()
    .websocket(8081)
    .unix("/tmp/mcp.sock")
    .grpc(50051)
    .with_tcp_nodelay(true)  // Disable Nagle's algorithm
    .run()
    .await?;
```

### SIMD-Accelerated JSON (v3)

Enable SIMD codec for faster JSON processing:

```toml
turbomcp = { version = "3.0", features = ["simd"] }
```

## Next Steps

- **[gRPC API](../api/grpc.md)** - gRPC transport details (v3)
- **[Wire Codecs](wire-codecs.md)** - Codec configuration (v3)
- **[Authentication](authentication.md)** - Add OAuth and security
- **[Observability](observability.md)** - Monitor transport metrics
- **[Examples](../examples/basic.md)** - Real-world transport usage
