# Transports

Configure and use multiple transport protocols for your MCP server.

## Overview

TurboMCP supports multiple transport protocols for different use cases:

- **STDIO** - Standard input/output for CLI integration
- **HTTP** - REST API with Server-Sent Events for server-to-client messages
- **WebSocket** - Full-duplex bidirectional communication
- **TCP** - Low-level TCP networking
- **Unix Sockets** - Local inter-process communication

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
    .run()
    .await?;
```

## Transport Details

### STDIO Transport

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

### HTTP Transport

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
- `POST /tools/call` - Call a tool
- `GET /tools/list` - List available tools
- `POST /resources/read` - Read a resource
- `GET /resources/list` - List resources
- `GET /events` - Server-Sent Events stream

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

### WebSocket Transport

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

### TCP Transport

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

### Unix Socket Transport

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

## Configuration

### Port Configuration

```rust
let server = McpServer::new()
    .http(8080)        // HTTP on port 8080
    .websocket(8081)   // WebSocket on port 8081
    .tcp(9000)         // TCP on port 9000
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

## Monitoring & Metrics

Get transport statistics:

```rust
let stats = server.transport_stats().await?;

println!("Active connections: {}", stats.active_connections);
println!("Total requests: {}", stats.total_requests);
println!("Error rate: {}%", stats.error_rate);
```

## Transport Selection Guide

| Transport | Latency | Throughput | Duplex | Use Case |
|-----------|---------|-----------|--------|----------|
| STDIO | Low | Medium | Half | CLI, local dev |
| HTTP | Medium | High | Half | Web, REST APIs |
| WebSocket | Low | Medium | Full | Real-time, interactive |
| TCP | Low | Very High | Full | High performance |
| Unix Socket | Very Low | Very High | Full | Local IPC |

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
// Reconnect on close
ws.onclose = () => {
    setTimeout(() => {
        ws = new WebSocket('ws://localhost:8081');
    }, 5000);  // Try again after 5 seconds
};
```

## Performance Tuning

### For High Throughput

Use TCP or WebSocket:

```rust
let server = McpServer::new()
    .tcp(9000)
    .websocket(8081)
    .with_buffer_size(1024 * 1024)  // 1MB buffers
    .run()
    .await?;
```

### For Low Latency

Use WebSocket or Unix Socket:

```rust
let server = McpServer::new()
    .websocket(8081)
    .unix("/tmp/mcp.sock")
    .with_tcp_nodelay(true)  // Disable Nagle's algorithm
    .run()
    .await?;
```

## Next Steps

- **[Authentication](authentication.md)** - Add OAuth and security
- **[Observability](observability.md)** - Monitor transport metrics
- **[Examples](../examples/basic.md)** - Real-world transport usage

