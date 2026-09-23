# Transports

Configure and use multiple transport protocols for your MCP server. TurboMCP v3 introduces modular transport crates for maximum flexibility.

## Overview

The `turbomcp` crate enables each server transport with a Cargo feature:

| Transport | Crate | Feature | Use Case |
|-----------|-------|---------|----------|
| **STDIO** | `turbomcp-stdio` | `stdio` (default) | CLI, Claude desktop |
| **Streamable HTTP** | `turbomcp-http` (client) / `turbomcp-server` (server) | `http` | Web applications, remote servers |
| **WebSocket** | `turbomcp-websocket` | `websocket` | Real-time bidirectional |
| **TCP** | `turbomcp-tcp` | `tcp` | High performance |
| **Unix** | `turbomcp-unix` | `unix` | Local IPC |
| **Channel** | `turbomcp-server` | `channel` | In-process testing and benchmarks |

gRPC lives in the standalone `turbomcp-grpc` crate with its own server type; it is
not a `turbomcp` feature. See the [gRPC API Reference](../api/grpc.md).

```toml
[dependencies]
turbomcp = { version = "3.5.0", features = ["full"] } # every transport + telemetry
```

## Basic Usage

Every `#[server]` type (every `McpHandler`) gets one `run_*` method per enabled
transport from the `McpHandlerExt` trait in the prelude, and a `builder()` for
configuration. The examples on this page use this server:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Get the weather for a city.
    #[tool]
    async fn get_weather(&self, city: String) -> String {
        format!("Sunny in {city}")
    }
}
```

### Single Transport (STDIO)

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_stdio().await
}
```

### Multiple Transports

A handler is `Clone`, so one server can listen on several transports at once by
running each on its own task:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    let server = MyServer;
    tokio::try_join!(
        server.run_http("0.0.0.0:8080"),      // Streamable HTTP
        server.run_websocket("0.0.0.0:8081"), // WebSocket
        server.run_tcp("0.0.0.0:9000"),       // TCP
    )?;
    Ok(())
}
```

### Runtime Selection

Choose the transport at startup with the builder:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    let transport = match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => Transport::http("0.0.0.0:8080"),
        Ok("ws") => Transport::websocket("0.0.0.0:8081"),
        Ok("tcp") => Transport::tcp("0.0.0.0:9000"),
        Ok("unix") => Transport::unix("/tmp/mcp.sock"),
        _ => Transport::stdio(),
    };
    MyServer.builder().transport(transport).serve().await
}
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
- Single client connection (the process that launched the server)
- Newline-delimited JSON-RPC on stdin/stdout, so all logging must go to stderr

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    // Keep stdout for the protocol
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    MyServer.run_stdio().await
}
```

## HTTP Transport

MCP's Streamable HTTP transport: one endpoint that takes JSON-RPC messages by
`POST`, streams server messages back as Server-Sent Events, and tracks each
client with an `Mcp-Session-Id` header.

**Use cases:**
- Web applications
- Remote and multi-client servers
- Cross-network communication

**Features:**
- Sessions (`Mcp-Session-Id`), with an idle timeout and session cap
- SSE for server-to-client requests and notifications, with `Last-Event-ID` resumption
- Origin validation, optional CORS, rate and connection limits
- MCP authorization (see [Authentication](authentication.md))

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_http("0.0.0.0:8080").await
}
```

**Endpoints:**

| Method | Path | Description |
|--------|------|-------------|
| POST | `/mcp` (or `/`) | Send a JSON-RPC request, notification, or response |
| GET | `/mcp` (or `/`) | Open the SSE stream for server-initiated messages |
| DELETE | `/mcp` (or `/`) | End the session |
| GET | `/.well-known/oauth-protected-resource…` | Protected Resource Metadata, when authorization is configured |

**Example client:**

```bash
# Initialize; the response carries the Mcp-Session-Id header to send on later requests
curl -i -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1.0"}}}'
```

A request from a non-loopback address with no `Origin` header (a CLI or
another server, rather than a browser) is refused unless you allow it; see
[Configuration](#configuration).

## WebSocket Transport

Full-duplex WebSocket for bidirectional real-time communication.

**Use cases:**
- Real-time applications
- Bidirectional elicitation and sampling
- High-frequency updates
- Interactive tools

**Features:**
- Full duplex communication
- Low latency
- One JSON-RPC message per WebSocket text frame

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_websocket("0.0.0.0:8081").await
}
```

**Connection URL:** `ws://localhost:8081` (also served at `/ws` and `/mcp/ws`)

**Example client (JavaScript):**

```javascript
const ws = new WebSocket('ws://localhost:8081');

ws.onopen = () => {
    ws.send(JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
            protocolVersion: '2025-11-25',
            capabilities: {},
            clientInfo: { name: 'browser', version: '1.0' }
        }
    }));
};

ws.onmessage = (event) => {
    const response = JSON.parse(event.data);
    console.log('Response:', response);
};
```

## TCP Transport

Raw TCP networking.

**Use cases:**
- High-performance scenarios
- Private networks
- Legacy system integration

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_tcp("0.0.0.0:9000").await
}
```

**Protocol:** JSON-RPC 2.0 messages separated by newlines. There is no
authentication or encryption on this transport; keep it on a private network.

## Unix Socket Transport

Local inter-process communication.

**Use cases:**
- Local service integration
- Docker containers
- Multi-process applications

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_unix("/tmp/mcp.sock").await // Create socket at path
}
```

Access control is the socket file's permissions.

## gRPC Transport (v3)

High-performance gRPC transport using tonic, in the standalone `turbomcp-grpc`
crate. It has its own server builder (`McpGrpcServer::builder()`) that takes tool,
resource, and prompt handlers directly, rather than running a `#[server]` type.
See the [gRPC API Reference](../api/grpc.md).

## Wire Codecs (v3)

`turbomcp-wire` provides pluggable codecs for code that encodes MCP messages
itself:

```rust
use turbomcp_wire::{Codec, JsonCodec, SimdJsonCodec};

// Standard JSON (default)
let codec = JsonCodec::new();

// SIMD-accelerated JSON (`simd` feature)
let codec = SimdJsonCodec::new();
```

The built-in transports do not take a codec: they speak JSON, as MCP requires.
See [Wire Codecs](wire-codecs.md).

## Configuration

The `run_*` methods use the default configuration. For anything else, use the
builder, or build a `ServerConfig`:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080"))
        .with_allowed_origin("https://app.example.com") // browser clients from this origin
        .with_rate_limit(100, Duration::from_secs(1))   // per client
        .with_connection_limit(1000)
        .with_max_message_size(4 * 1024 * 1024)         // default 10 MiB
        .with_graceful_shutdown(Duration::from_secs(30)) // HTTP: drain in-flight requests
        .serve()
        .await
}
```

`ServerConfig` covers the rest, including CORS, requests without an `Origin`
header, and the HTTP session policy:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    let config = ServerConfig::builder()
        .allow_origin("https://app.example.com")
        .cors(true)                    // answer CORS preflights for allowed origins
        .allow_missing_origin(true)    // accept CLIs and other servers (pair with auth)
        .http_session_idle_timeout(Duration::from_secs(15 * 60))
        .build();

    // `with_config` does not carry the HTTP session policy or authorization;
    // run the HTTP transport with the config directly to keep them.
    turbomcp_server::transport::http::run_with_config(&MyServer, "0.0.0.0:8080", &config)
        .await
}
```

### TLS/SSL

The server transports do not terminate TLS. Put a reverse proxy (nginx, Envoy,
a cloud load balancer) in front of the HTTP or WebSocket transport, or serve the
Axum router from `MyServer.builder().into_axum_router()` with a TLS-capable
server such as `axum-server`.

### Embedding in an Axum App

```rust
use axum::{Router, routing::get};
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(MyServer.builder().into_axum_router());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

## Connection Management

### Graceful Shutdown

The HTTP transport stops on Ctrl-C (and SIGTERM on Unix) and waits up to the
configured time for in-flight requests. A STDIO server exits when stdin closes.

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080"))
        .with_graceful_shutdown(Duration::from_secs(30))
        .serve()
        .await
}
```

### Client-Side Resilience

Retries and circuit breaking are client concerns: see
`ClientBuilder::build_resilient` in the [Client API](../api/client.md).

## Transport Selection Guide

| Transport | Latency | Throughput | Duplex | Best For |
|-----------|---------|-----------|--------|----------|
| STDIO | Low | Medium | Full | CLI, local dev |
| HTTP | Medium | High | Full (POST + SSE) | Web, remote servers |
| WebSocket | Low | Medium | Full | Real-time, interactive |
| TCP | Low | Very High | Full | High performance, private networks |
| Unix Socket | Very Low | Very High | Full | Local IPC |
| gRPC | Low | Very High | Full | Enterprise, microservices |

## Using Individual Transport Crates

The transport crates (`turbomcp-stdio`, `turbomcp-http`, `turbomcp-websocket`,
`turbomcp-tcp`, `turbomcp-unix`) implement the `Transport` trait that the
*client* runs on. Most applications reach them through `turbomcp-client`, which
re-exports each one behind a feature:

```rust
use std::net::SocketAddr;
use turbomcp_client::{Client, StreamableHttpClientConfig, StreamableHttpClientTransport, TcpTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let http = StreamableHttpClientTransport::new(StreamableHttpClientConfig {
        base_url: "http://localhost:8080".to_string(),
        ..Default::default()
    })?;
    let http_client = Client::new(http);

    let server: SocketAddr = "127.0.0.1:9000".parse()?;
    let tcp = TcpTransport::new_client("0.0.0.0:0".parse()?, server);
    let tcp_client = Client::new(tcp);
    Ok(())
}
```

## Monitoring & Metrics

Enable the `telemetry` feature for OpenTelemetry tracing and Prometheus metrics.
See [Observability](observability.md).

## Troubleshooting

### "Address already in use"

The port is already bound. Pick another one, or stop the process holding it.

### HTTP 403 for a CLI or server client

The request came from a non-loopback address without an `Origin` header. Set
`allow_missing_origin(true)` on the `ServerConfig`, together with authorization.

### Connection timeouts

Client request timeouts are set on the client
(`ClientBuilder::with_timeout`, or `client.with_timeout(duration)` for one call).

### WebSocket connection drops

Client may not support long connections. Implement reconnection:

```javascript
ws.onclose = () => {
    setTimeout(() => {
        ws = new WebSocket('ws://localhost:8081');
    }, 5000);
};
```

## Performance Tuning

### For High Throughput

Use TCP, Unix sockets, or WebSocket, and raise the connection limit to match
your expected concurrency (`with_connection_limit`).

### For Low Latency

Use a Unix socket for local clients, or WebSocket for remote ones.

### SIMD-Accelerated JSON (v3)

`turbomcp-protocol` enables SIMD JSON (`sonic-rs`) through its default `simd`
feature, so the server gets it without extra configuration. For explicit codec
control in your own code, use `turbomcp-wire`:

```toml
turbomcp-wire = { version = "3.5.0", features = ["simd"] }
```

## Next Steps

- **[gRPC API](../api/grpc.md)** - gRPC transport details (v3)
- **[Wire Codecs](wire-codecs.md)** - Codec configuration (v3)
- **[Authentication](authentication.md)** - Add OAuth and security
- **[Observability](observability.md)** - Monitor transport metrics
- **[Examples](../examples/basic.md)** - Real-world transport usage
