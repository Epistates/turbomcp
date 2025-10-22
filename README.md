# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Tests](https://github.com/Epistates/turbomcp/actions/workflows/test.yml/badge.svg)](https://github.com/Epistates/turbomcp/actions/workflows/test.yml)
[![Security](https://img.shields.io/badge/Security-Audited-green.svg)](./crates/turbomcp-transport/SECURITY_FEATURES.md)
[![Performance](https://img.shields.io/badge/Performance-Benchmarked-brightgreen.svg)](./benches/)

**Production-ready Rust SDK for the Model Context Protocol (MCP) with zero-boilerplate development and progressive enhancement.**

Build MCP servers in seconds with automatic schema generation, type-safe handlers, and multiple transport protocols.

## Table of Contents

- [60-Second Quick Start](#60-second-quick-start)
- [Why TurboMCP?](#why-turbomcp)
- [Key Features](#key-features)
- [Quick Start](#quick-start)
- [Client Connections](#client-connections-v20)
- [Type-State Capability Builders](#type-state-capability-builders)
- [Additional Features](#additional-features)
  - [AudioContent Support](#audiocontent-support)
  - [LLM Integration](#llm-integration)
  - [Interactive Forms](#interactive-forms)
  - [Transport Protocols](#transport-protocols)
  - [Client Cloning & Shared Transport](#client-cloning--shared-transport)
  - [Filesystem Security](#filesystem-security)
  - [Resource Templates & Dynamic Content](#resource-templates--dynamic-content)
- [Security Features](#security-features)
- [Performance](#performance)
- [Deployment & Operations](#deployment--operations)
- [Development & Testing](#development--testing)
- [Example: Production Server](#example-production-server)
- [Architecture & Design Philosophy](#architecture--design-philosophy)
- [Documentation & Resources](#documentation--resources)
- [Contributing](#contributing)
- [License](#license)
- [Status](#status)

---

## 60-Second Quick Start

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(name = "calculator", version = "1.0.0", transports = ["stdio"])]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

**That's it.** Save as `main.rs`, run `cargo run`, and connect from Claude Desktop.

**[Examples (26)](crates/turbomcp/examples/)** | **[API Docs](https://docs.rs/turbomcp)** | **[Transport Patterns](crates/turbomcp/examples/TRANSPORT_EXAMPLES_README.md)**

---

## Why TurboMCP?

### ✨ Zero Boilerplate

Automatic JSON schema generation from function signatures. No manual schema construction, argument parsing, or result wrapping.

### 🚀 Progressive Enhancement

Start minimal (STDIO only), add features as needed:
- HTTP/SSE for web integration
- WebSocket for bidirectional streaming
- TCP for network services
- OAuth 2.1 for authentication
- SIMD for high throughput

### 🔒 Production Ready

- Zero known runtime vulnerabilities
- Comprehensive error handling
- Session management with cleanup
- Rate limiting and security headers
- Type-safe from compile to runtime

### 🎯 Developer Experience

- Context injection for dependencies
- Interactive elicitation forms
- Automatic logging and tracing
- 25 focused examples covering all patterns
- Comprehensive documentation

---

## Key Features

**Procedural Macro System**
- `#[server]` - Zero-boilerplate server definition with capability configuration
- `#[tool]` - Tool handlers with automatic parameter validation and schema generation
- `#[resource]` - Resource handlers with URI template support
- `#[prompt]` - Dynamic prompt templates with parameter substitution
- `#[completion]` - Autocompletion handlers for intelligent suggestions
- `#[ping]` - Health check endpoints with custom logic
- `#[elicitation]` - Interactive form builders with validation
- `mcp_error!` - Ergonomic error creation with context
- `elicit!` - Simple elicitation for quick user input

**Type-State Architecture**
- ServerCapabilitiesBuilder with compile-time validation
- ClientCapabilitiesBuilder for capability negotiation
- Type-safe capability dependencies (e.g., tool_list_changed only when tools enabled)
- SharedTransport for concurrent transport access (Client is directly cloneable)
- Clone pattern for McpServer and Client (Axum/Tower standard - cheap Arc increments)

**Protocol Features**
- Full MCP 2025-06-18 specification compliance
- Server-initiated sampling for bidirectional AI communication
- Interactive form elicitation with real-time validation
- AudioContent support for multimedia applications
- Progress tracking with correlation IDs
- Resource templates with RFC 6570 URI patterns

**Transport & Performance**
- **MCP Standard Transports**: STDIO, HTTP/SSE (full 2025-06-18 spec compliance)
- **Custom Extensions**: WebSocket, TCP, Unix Socket (MCP-compliant bidirectional transports)
- SIMD-accelerated JSON processing (simd-json, sonic-rs)
- Zero-copy message handling with Bytes
- Circuit breakers, connection pooling, and graceful degradation
- Built-in benchmarking suite with 5% regression detection

**Security & Authentication**
- OAuth 2.0/2.1 with PKCE and multi-provider support (Google, GitHub, Microsoft)
- Rate limiting with token bucket algorithm
- CORS protection and comprehensive security headers
- Session management with cleanup and timeout enforcement
- TLS support with certificate validation

**Developer Tools**
- Context injection with dependency resolution
- Automatic JSON schema generation from Rust types
- CLI tools for testing, debugging, and schema export
- Graceful shutdown with lifecycle management
- Comprehensive error types with structured context

---

## Quick Start

```toml
[dependencies]
turbomcp = "2.0"
tokio = { version = "1.0", features = ["full"] }
serde_json = "1.0"
```

Create an MCP server with zero boilerplate:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct HelloServer;

#[server(name = "hello-server", version = "1.0.0")]
impl HelloServer {
    #[tool("Say hello to someone")]
    async fn hello(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {name}! Welcome to TurboMCP! 🦀⚡"))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    HelloServer.run_stdio().await?;
    Ok(())
}
```

Configure in Claude Desktop:
```json
{
  "mcpServers": {
    "hello-server": {
      "command": "/path/to/your/server",
      "args": []
    }
  }
}
```

Test with CLI:
```bash
cargo install turbomcp-cli
turbomcp-cli tools list --command "./your-server"
turbomcp-cli tools call hello --arguments '{"name": "World"}' --command "./your-server"
```

---

## Client Connections (v2.0)

TurboMCP v2.0 provides beautiful one-liner client connections with automatic initialization:

### HTTP Client
```rust
use turbomcp_client::Client;

// One-liner connection (auto-connects & initializes)
let client = Client::connect_http("http://localhost:8080").await?;

// Now use it immediately
let tools = client.list_tools().await?;
let result = client.call_tool("my_tool", Some(args)).await?;
```

### TCP Client
```rust
// Connect to TCP server
let client = Client::connect_tcp("127.0.0.1:8765").await?;
let tools = client.list_tools().await?;
```

### Unix Socket Client
```rust
// Connect to Unix socket server
let client = Client::connect_unix("/tmp/mcp.sock").await?;
let prompts = client.list_prompts().await?;
```

### Custom Configuration
```rust
use std::time::Duration;

// HTTP with custom config
let client = Client::connect_http_with("http://localhost:8080", |config| {
    config.timeout = Duration::from_secs(60);
    config.endpoint_path = "/api/mcp".to_string();
}).await?;

// WebSocket with custom config
let client = Client::connect_websocket_with("ws://localhost:8080/ws", |config| {
    config.reconnect_attempts = 5;
    config.ping_interval = Duration::from_secs(30);
}).await?;
```

### Manual Connection (Advanced)
For full control over initialization:

```rust
use turbomcp_transport::tcp::TcpTransport;

let transport = TcpTransport::new_client(bind_addr, server_addr);
let client = Client::new(transport);

// Auto-connects transport during initialize
client.initialize().await?;

// Use client
let tools = client.list_tools().await?;
```

### Benefits of v2.0 Client API
- **One-liner connections**: `connect_http()`, `connect_tcp()`, `connect_unix()`
- **Auto-initialization**: No need to call `.connect()` or `.initialize()` manually
- **Type-safe configuration**: Custom config functions with full IntelliSense
- **Consistent API**: Same pattern across all transports

---

## Type-State Capability Builders

Compile-time validated capability configuration:

```rust
use turbomcp_protocol::capabilities::builders::{ServerCapabilitiesBuilder, ClientCapabilitiesBuilder};

// Server capabilities with compile-time validation
let server_caps = ServerCapabilitiesBuilder::new()
    .enable_tools()                    // Enable tools capability
    .enable_prompts()                  // Enable prompts capability
    .enable_resources()                // Enable resources capability
    .enable_tool_list_changed()        // ✅ Only available when tools enabled
    .enable_resources_subscribe()      // ✅ Only available when resources enabled
    .build();

// Client capabilities with opt-out model (all enabled by default)
let client_caps = ClientCapabilitiesBuilder::new()
    .enable_roots_list_changed()       // Configure sub-capabilities
    .build();                          // All capabilities enabled!

// Opt-in pattern for restrictive clients
let minimal_client = ClientCapabilitiesBuilder::minimal()
    .enable_sampling()                 // Only enable what you need
    .enable_roots()
    .build();
```

### Benefits
- Compile-time validation catches invalid configurations at build time
- Type safety ensures sub-capabilities are only available when parent capability is enabled
- Fluent API for readable capability configuration
- Backward compatibility with existing code

---

## Additional Features

### AudioContent Support
Multimedia content handling with the AudioContent type:

```rust
#[tool("Process audio data")]
async fn process_audio(&self, audio_data: String) -> McpResult<Content> {
    Ok(Content::Audio(AudioContent {
        data: audio_data,  // Base64 encoded audio
        mime_type: "audio/wav".to_string(),
        annotations: Some(Annotations {
            audience: Some(vec!["user".to_string()]),
            priority: Some(0.9),
            last_modified: Some(iso8601_timestamp()),
        }),
    }))
}
```

### LLM Integration
Server-initiated sampling for AI communication:

```rust
// Server can request LLM assistance through the client
#[tool("Get AI assistance for data analysis")]
async fn ai_analyze(&self, ctx: Context, data: String) -> McpResult<String> {
    // Create sampling request for client's LLM
    let request = serde_json::json!({
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": format!("Analyze this data: {}", data)
            }
        }],
        "maxTokens": 500,
        "systemPrompt": "You are a data analyst."
    });

    // Request LLM assistance through client
    match ctx.create_message(request).await {
        Ok(response) => Ok(format!("AI Analysis: {:?}", response)),
        Err(_) => {
            // Fallback if sampling unavailable
            Ok(format!("Static analysis: {} characters", data.len()))
        }
    }
}
```

### Interactive Forms
Server-initiated user input with validation:

```rust
let config = ctx.elicit("Deployment Configuration")
    .field("environment",
        select("Environment", vec!["dev", "staging", "production"])
            .description("Target deployment environment"))
    .field("replicas",
        integer("Replica Count").range(1.0, 20.0).default(3))
    .field("auto_scale",
        checkbox("Enable Auto-scaling").default(true))
    .field("notification_email",
        text("Admin Email").format("email").required())
    .section("Security")
    .field("enable_tls", checkbox("Enable TLS").default(true))
    .field("cors_origins", text("CORS Origins").placeholder("https://app.example.com"))
    .require(vec!["environment", "notification_email"])
    .validate_with(|data| {
        if data.get::<String>("environment")? == "production" && !data.get::<bool>("enable_tls")? {
            return Err("TLS required for production".into());
        }
        Ok(())
    })
    .await?;
```

### Transport Protocols

| Transport | Performance | Use Case | Security |
|-----------|-------------|----------|----------|
| **STDIO** | Fast | Claude Desktop, CLI tools | Process isolation |
| **HTTP/SSE** | Good | Web apps, REST APIs | TLS 1.3, session mgmt |
| **WebSocket** | Real-time | Interactive apps | Secure WebSocket |
| **TCP** | High throughput | Clusters | Optional TLS |
| **Unix Socket** | Fast | Container communication | File permissions |

```rust
// Transport selection
match deployment_env {
    "cluster" => server.run_tcp("0.0.0.0:8080").await?,
    "container" => server.run_unix("/var/run/mcp.sock").await?,
    "web" => server.run_http("0.0.0.0:8080").await?,
    "desktop" => server.run_stdio().await?,
    _ => server.run_stdio().await?, // Default to stdio
}
```

---

## Security Features

### Security Architecture
TurboMCP includes security features for production deployment:

- **CORS Protection**: Configurable origin policies with production-safe defaults
- **Security Headers**: Standard HTTP security headers (X-Frame-Options, CSP, HSTS)
- **Session Management**: Secure session handling with timeout enforcement
- **TLS Support**: Optional TLS for transport protocols
- **OAuth 2.1**: Via optional `auth` feature (API key, Bearer token authentication)
- **DPoP Support**: RFC 9449 Demonstration of Proof-of-Possession (via `dpop` feature)
- **Rate Limiting**: Request throttling configuration
- **Type-Safe Errors**: Structured error handling with context

```rust
// Example: Enable OAuth 2.1 authentication
#[cfg(feature = "auth")]
use turbomcp::auth::*;

// Configure with CORS security
let security = SecurityConfig::production()
    .with_origins(vec!["https://app.example.com".to_string()]);

// See crates/turbomcp-transport/SECURITY_FEATURES.md for complete security documentation
```

### Monitoring & Observability
```rust
#[tool("Get system metrics")]
async fn get_metrics(&self, ctx: Context) -> McpResult<SystemMetrics> {
    // Structured logging with correlation IDs
    ctx.with_trace_id().info("Metrics requested").await?;

    // Performance monitoring
    let metrics = SystemMetrics {
        requests_per_second: self.metrics.current_rps(),
        average_latency: self.metrics.avg_latency(),
        error_rate: self.metrics.error_rate(),
        memory_usage: self.metrics.memory_usage(),
        cpu_utilization: self.metrics.cpu_usage(),
        active_connections: self.transport.active_connections(),
    };

    // Export to Prometheus/Grafana
    self.metrics.export_prometheus().await?;

    Ok(metrics)
}
```

---

## Performance

### Benchmark Results

```
TurboMCP Performance Characteristics
==================================================
Message Throughput:     Concurrent processing  (TCP transport)
Tool Execution:         Fast response times    (99th percentile)
JSON Processing:        SIMD-accelerated      (simd-json, sonic-rs)
Memory Efficiency:      Zero-copy patterns    (Bytes-based processing)
Cold Start Time:        Fast startup          (Pre-compiled schemas)
Connection Setup:       Fast connection       (Connection pooling)

Regression Detection: Enabled
Cross-Platform Validation: Ubuntu | Windows | macOS
CI/CD Performance Gates: 5% threshold
```

### Low-Overhead Architecture
```rust
// Compile-time schema generation (zero runtime cost)
#[tool("Process order")]
async fn process_order(
    &self,
    #[description("Customer order ID")] order_id: String,
    #[description("Priority level 1-10")] priority: u8,
    #[description("Processing options")] options: ProcessingOptions,
) -> McpResult<OrderResult> {
    // Schema validation happens at compile time
    // Handler dispatch is O(1) lookup
    // Zero reflection overhead
    // No runtime schema computation

    let result = self.order_processor
        .process_with_priority(order_id, priority, options)
        .await?;

    Ok(result)
}

// Automatic JSON schema generated at compile time:
// {
//   "type": "object",
//   "properties": {
//     "order_id": {"type": "string", "description": "Customer order ID"},
//     "priority": {"type": "integer", "minimum": 0, "maximum": 255},
//     "options": {"$ref": "#/definitions/ProcessingOptions"}
//   },
//   "required": ["order_id", "priority", "options"]
// }
```

---

## Additional Features

### Client Cloning & Shared Transport
Thread-safe async operations with Arc-wrapped internals:

```rust
// Client is directly cloneable (Arc-wrapped internally - no wrapper needed!)
let client = Client::connect_http("http://localhost:8080").await?;

// Clone for concurrent usage (cheap Arc increments)
let c1 = client.clone();
let c2 = client.clone();
let c3 = client.clone();

// All tasks can access concurrently
let results = tokio::try_join!(
    async move { c1.list_tools().await },
    async move { c2.list_prompts().await },
    async move { c3.list_resources().await },
)?;

// For transport sharing across multiple connections, use SharedTransport
let transport = TcpTransport::connect("server:8080").await?;
let shared_transport = SharedTransport::new(transport);
shared_transport.connect().await?;

// Multiple clients sharing the same transport
let client1 = Client::new(shared_transport.clone());
let client2 = Client::new(shared_transport.clone());
let client3 = Client::new(shared_transport.clone());
```

### Filesystem Security
```rust
#[server(
    name = "secure-file-server",
    version = "1.0.0",
    root = "file:///workspace:Project Files",
    root = "file:///uploads:User Uploads",
    root = "file:///tmp:Temporary Files"
)]
impl SecureFileServer {
    #[tool("Read file within security boundaries")]
    async fn read_file(&self, ctx: Context, path: String) -> McpResult<String> {
        // Automatic path validation against configured roots
        // OS-aware security (Unix permissions, Windows ACLs)
        ctx.validate_file_access(&path).await?;

        let content = tokio::fs::read_to_string(&path).await
            .map_err(|e| mcp_error!("Access denied: {}", e))?;

        Ok(content)
    }
}
```

### Resource Templates & Dynamic Content
```rust
#[resource("user://{user_id}/profile")]
async fn get_user_profile(&self, user_id: String) -> McpResult<Content> {
    let profile = self.database.get_user_profile(&user_id).await?;

    Ok(Content::Text(TextContent {
        text: serde_json::to_string_pretty(&profile)?,
        mime_type: Some("application/json".to_string()),
        annotations: Some(Annotations {
            audience: Some(vec!["user".to_string(), "admin".to_string()]),
            priority: Some(0.8),
            last_modified: Some(profile.updated_at.to_rfc3339()),
        }),
    }))
}

#[resource("file://{path}")]
async fn serve_file(&self, path: String) -> McpResult<Content> {
    // Automatic MIME type detection
    // Security validation
    // Efficient file serving
    self.file_server.serve_secure(&path).await
}
```

---

## Deployment & Operations

### Container Deployment
```dockerfile
FROM rust:1.89 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --features production

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/your-server /usr/local/bin/
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1
CMD ["your-server"]
```

### Kubernetes Deployment
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: turbomcp-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: turbomcp-server
  template:
    metadata:
      labels:
        app: turbomcp-server
    spec:
      containers:
      - name: server
        image: your-registry/turbomcp-server:v1.0.0
        ports:
        - containerPort: 8080
        env:
        - name: TRANSPORT
          value: "http"
        - name: RUST_LOG
          value: "info"
        resources:
          requests:
            memory: "64Mi"
            cpu: "50m"
          limits:
            memory: "256Mi"
            cpu: "500m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 30
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 10
```

### Monitoring
```rust
// Prometheus metrics integration
#[tool("Process critical operation")]
async fn critical_operation(&self, ctx: Context, data: String) -> McpResult<String> {
    let _timer = ctx.start_timer("critical_operation_duration");
    ctx.increment_counter("critical_operations_total");

    match self.process_data(data).await {
        Ok(result) => {
            ctx.increment_counter("critical_operations_success");
            ctx.record_histogram("result_size_bytes", result.len() as f64);
            Ok(result)
        }
        Err(e) => {
            ctx.increment_counter("critical_operations_error");
            ctx.add_label("error_type", &e.error_type());
            Err(e)
        }
    }
}

// Grafana dashboard query examples:
// rate(critical_operations_total[5m])           # Operations per second
// histogram_quantile(0.95, critical_operation_duration)  # 95th percentile latency
// critical_operations_error / critical_operations_total  # Error rate
```

---

## Development & Testing

### Testing
```bash
# Full test suite with performance validation
make test                           # 81 tests, zero failures
cargo test --workspace --all-features  # All features tested
cargo bench --workspace            # Performance benchmarks

# Security & quality validation
cargo audit                         # Zero vulnerabilities
cargo clippy --all-targets         # Zero warnings
cargo deny check                    # License compliance

# Performance regression detection
./scripts/run_benchmarks.sh ci      # CI-optimized run
./scripts/run_benchmarks.sh baseline  # Update baselines
```

### CLI Development Tools
```bash
# Install CLI tools
cargo install turbomcp-cli

# Test your server during development
turbomcp-cli tools list --command "./target/debug/your-server"
turbomcp-cli tools call process_data --arguments '{"input": "test data"}' \
  --command "./target/debug/your-server"

# Export schemas for documentation
turbomcp-cli tools schema --command "./target/debug/your-server" --format pretty

# List resources and prompts
turbomcp-cli resources list --command "./target/debug/your-server"
turbomcp-cli prompts list --command "./target/debug/your-server"
```

### Examples - All 26

**Foundational Examples:**
| Example | Topic |
|---------|-------|
| [hello_world.rs](./crates/turbomcp/examples/hello_world.rs) | Simplest server - one tool |
| [macro_server.rs](./crates/turbomcp/examples/macro_server.rs) | Using `#[server]` macro |
| [minimal_test.rs](./crates/turbomcp/examples/minimal_test.rs) | Testing patterns |
| [tools.rs](./crates/turbomcp/examples/tools.rs) | Parameter types & validation |
| [resources.rs](./crates/turbomcp/examples/resources.rs) | Resource handlers with URIs |
| [rich_tool_descriptions.rs](./crates/turbomcp/examples/rich_tool_descriptions.rs) | Advanced tool documentation |
| [validation.rs](./crates/turbomcp/examples/validation.rs) | Input validation & error handling |

**Stateful & Advanced Server Examples:**
| Example | Topic |
|---------|-------|
| [stateful.rs](./crates/turbomcp/examples/stateful.rs) | Arc<RwLock<T>> state pattern |
| [sampling_server.rs](./crates/turbomcp/examples/sampling_server.rs) | LLM sampling & bidirectional communication |
| [elicitation_server.rs](./crates/turbomcp/examples/elicitation_server.rs) | Interactive form elicitation |

**Transport & Server Patterns:**
| Example | Topic |
|---------|-------|
| [stdio_app.rs](./crates/turbomcp/examples/stdio_app.rs) | Complete STDIO application |
| [stdio_server.rs](./crates/turbomcp/examples/stdio_server.rs) | STDIO server transport |
| [http_app.rs](./crates/turbomcp/examples/http_app.rs) | Complete HTTP/SSE application |
| [http_server.rs](./crates/turbomcp/examples/http_server.rs) | HTTP/SSE transport only |
| [tcp_server.rs](./crates/turbomcp/examples/tcp_server.rs) | TCP network transport |
| [unix_server.rs](./crates/turbomcp/examples/unix_server.rs) | Unix socket transport |
| [websocket_server.rs](./crates/turbomcp/examples/websocket_server.rs) | WebSocket bidirectional transport |
| [transports_demo.rs](./crates/turbomcp/examples/transports_demo.rs) | Multi-transport demonstration |

**Client Examples:**
| Example | Topic |
|---------|-------|
| [basic_client.rs](./crates/turbomcp/examples/basic_client.rs) | Connect, list, and call tools |
| [stdio_client.rs](./crates/turbomcp/examples/stdio_client.rs) | STDIO client communication |
| [tcp_client.rs](./crates/turbomcp/examples/tcp_client.rs) | TCP client connection |
| [http_client_simple.rs](./crates/turbomcp/examples/http_client_simple.rs) | HTTP/SSE client |
| [unix_client.rs](./crates/turbomcp/examples/unix_client.rs) | Unix socket client |
| [websocket_client_simple.rs](./crates/turbomcp/examples/websocket_client_simple.rs) | WebSocket client |
| [comprehensive.rs](./crates/turbomcp/examples/comprehensive.rs) | All MCP features combined |
| [elicitation_client.rs](./crates/turbomcp/examples/elicitation_client.rs) | Interactive form responses |

**Run examples:**
```bash
cargo run --example hello_world
cargo run --example elicitation_client
cargo run --example http_app
cargo run --example tcp_server  # Then in another terminal:
cargo run --example tcp_client
```

---

## Example: Production Server

Create a server with multiple features:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct ProductionServer {
    database: Arc<Database>,
    cache: Arc<Cache>,
}

#[server(
    name = "enterprise-server",
    version = "1.0.0",
    description = "Production server with advanced features",
    capabilities = ServerCapabilities::builder()
        .enable_tools()
        .enable_prompts()
        .enable_resources()
        .enable_tool_list_changed()
        .enable_resources_subscribe()
        .build()
)]
impl ProductionServer {
    #[tool("Analyze data with AI assistance")]
    async fn ai_analyze(&self, ctx: Context, data: String) -> McpResult<String> {
        // Enterprise logging with correlation IDs
        ctx.info(&format!("Processing {} bytes", data.len())).await?;

        // Production database query with connection pooling
        let metadata = self.database.get_metadata(&data).await?;

        // Request AI assistance through client sampling
        let request = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!("Analyze: {}", data)
                }
            }],
            "maxTokens": 500,
            "systemPrompt": "You are a data analyst."
        });

        match ctx.create_message(request).await {
            Ok(response) => Ok(format!("AI Analysis: {:?}", response)),
            Err(_) => {
                // Fallback analysis
                let word_count = data.split_whitespace().count();
                Ok(format!("Analysis: {} words, {} bytes", word_count, data.len()))
            }
        }
    }

    #[tool("Interactive data collection")]
    async fn collect_requirements(&self, ctx: Context) -> McpResult<String> {
        // Server-initiated interactive form with validation
        let response = ctx.elicit("Project Requirements")
            .field("project_name", text("Project Name").min_length(3))
            .field("budget", integer("Budget ($)").range(1000.0, 1000000.0))
            .field("deadline", text("Deadline").format("date"))
            .field("priority", select("Priority", vec!["low", "medium", "high", "critical"]))
            .require(vec!["project_name", "budget"])
            .await?;

        // Process with type-safe extraction
        let name = response.get::<String>("project_name")?;
        let budget = response.get::<i64>("budget")?;

        Ok(format!("Project '{}' with budget ${} configured", name, budget))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = ProductionServer {
        database: Arc::new(Database::connect().await?),
        cache: Arc::new(Cache::new()),
    };

    // Production deployment with automatic transport selection
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => server.run_http("0.0.0.0:8080").await?,
        Ok("tcp") => server.run_tcp("0.0.0.0:8080").await?,
        _ => server.run_stdio().await?, // Claude Desktop integration
    }

    Ok(())
}
```

---

## Architecture & Design Philosophy

### Compile-Time Optimization Approach

TurboMCP prioritizes compile-time optimization over runtime flexibility. This creates a more complex build process but delivers better runtime performance and predictability.

### Modular Design

| Crate | Purpose | Key Innovation |
|-------|---------|----------------|
| [`turbomcp`](./crates/turbomcp/) | Main SDK | Zero-overhead macro integration |
| [`turbomcp-protocol`](./crates/turbomcp-protocol/) | Protocol & Foundation | SIMD-accelerated processing + compile-time schemas |
| [`turbomcp-transport`](./crates/turbomcp-transport/) | Transport layer | High throughput with circuit breakers |
| [`turbomcp-server`](./crates/turbomcp-server/) | Server framework | Enterprise security & middleware |
| [`turbomcp-client`](./crates/turbomcp-client/) | Client library | Advanced LLM integration |
| [`turbomcp-macros`](./crates/turbomcp-macros/) | Proc macros | Compile-time optimization engine |
| [`turbomcp-cli`](./crates/turbomcp-cli/) | CLI tools | Production testing & debugging |

**Note:** In v2.0.0, `turbomcp-core` was merged into `turbomcp-protocol` to eliminate circular dependencies.

### Performance Characteristics

- Low-overhead abstractions - Optimizations happen at compile time
- O(1) handler dispatch - Direct function calls, no HashMap lookups
- Pre-computed schemas - No runtime schema generation overhead
- Optimized JSON processing - SIMD-accelerated parsing with simd-json and sonic-rs
- Zero-copy message handling - Minimal memory allocations with `Bytes`
- Feature gating - Lean production binaries through compile-time selection

---

## Documentation & Resources

- **[API Documentation](https://docs.rs/turbomcp)** - Complete API reference with examples
- **[Benchmarking Guide](./benches/README.md)** - Performance testing and optimization
- **[Security Documentation](./crates/turbomcp-transport/SECURITY_FEATURES.md)** - Enterprise security features
- **[Architecture Guide](./ARCHITECTURE.md)** - System design and component interaction
- **[Examples Guide](./crates/turbomcp/examples/README.md)** - 26 focused examples with learning paths
- **[Transport Patterns](./crates/turbomcp/examples/TRANSPORT_EXAMPLES_README.md)** - TCP, HTTP, WebSocket, Unix socket patterns
- **[MCP Specification](https://modelcontextprotocol.io)** - Official protocol documentation
- **[Migration Guide](./MIGRATION.md)** - v1.x to v2.0 changes

---

## Contributing

We welcome contributions! TurboMCP follows high engineering standards:

1. **Fork the repository** and create a feature branch
2. **Write comprehensive tests** - We maintain 100% test success rate
3. **Run the quality suite** - `make test && cargo clippy && cargo fmt`
4. **Ensure security compliance** - `cargo audit && cargo deny check`
5. **Submit a pull request** with detailed description

**Development setup:**
```bash
git clone https://github.com/Epistates/turbomcp.git
cd turbomcp
cargo build --workspace
make test                    # Run full test suite
./scripts/run_benchmarks.sh  # Validate performance
```

---

## 📄 License

Licensed under the [MIT License](./LICENSE) - Enterprise-friendly open source.

---

## Status

TurboMCP v2.0.4 provides:

- Zero known security vulnerabilities with continuous monitoring
- Performance focus with automated regression detection
- Full MCP 2025-06-18 specification compliance
- Production deployment patterns with container & Kubernetes support
- 26 focused examples covering all usage patterns
- Active development with regular security updates and performance improvements

**Production Status:** TurboMCP 2.0.4 is production-ready with full MCP 2025-06-18 compliance and comprehensive test coverage. The API is stable.

---

*Built with ❤️ by the TurboMCP team
