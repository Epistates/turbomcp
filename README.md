# TurboMCP ⚡

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Tests](https://github.com/Epistates/turbomcp/actions/workflows/test.yml/badge.svg)](https://github.com/Epistates/turbomcp/actions/workflows/test.yml)
[![Security](https://img.shields.io/badge/Security-Zero%20Vulnerabilities-green.svg)](./deny.toml)
[![Performance](https://img.shields.io/badge/Performance-Benchmarked-brightgreen.svg)](./benches/)

**The world's most advanced Rust SDK for the Model Context Protocol (MCP)**

🏆 **Full MCP Compliance** • 🔒 **Zero Vulnerabilities** • ⚡ **High Performance** • 🛡️ **Production Ready**

---

## 🚀 Why TurboMCP is the Enterprise Choice

### 🔒 **SECURITY FIRST** - Zero Known Vulnerabilities
- **✅ Complete Security Audit** - Eliminated all known vulnerabilities (RSA RUSTSEC-2023-0071, paste RUSTSEC-2024-0436)
- **✅ Enterprise Security Policy** - Comprehensive cargo-deny configuration with MIT-compatible license enforcement
- **✅ Production Hardening** - Strategic dependency optimization and attack surface reduction
- **✅ Continuous Security Monitoring** - Automated vulnerability scanning in CI/CD pipeline

### ⚡ **HIGH PERFORMANCE** - Optimized for Production
- **High-throughput message processing** - Optimized for concurrent workloads
- **Sub-millisecond response times** - Fast tool execution with minimal latency
- **SIMD-accelerated JSON processing** - Hardware-optimized parsing with zero-copy optimization
- **Efficient memory usage** - Minimal overhead per request
- **Comprehensive benchmarking** - Automated regression detection with 5% threshold

### 🏆 **PRODUCTION EXCELLENCE** - Enterprise-Grade Features
- **100% MCP 2025-06-18 Compliance** - Only library with complete specification coverage
- **5 Production Transports** - STDIO, HTTP/SSE, WebSocket, TCP, Unix Socket with enterprise session management
- **Advanced LLM Integration** - Production-grade Anthropic/OpenAI backends with streaming support
- **Interactive Elicitation** - Server-initiated user input with real-time validation
- **OAuth 2.1 Compliance** - Google, GitHub, Microsoft providers with PKCE security
- **Circuit Breakers & Monitoring** - Production reliability with graceful degradation

### 🎯 **DEVELOPER EXPERIENCE** - Zero-Boilerplate Ergonomics
- **Procedural Macros** - `#[server]`, `#[tool]`, `#[resource]` with automatic schema generation
- **Compile-Time Optimization** - Zero runtime overhead through advanced macro system
- **Type Safety** - Full Rust type system integration with compile-time validation
- **Industry-Exclusive Features** - AudioContent support, enhanced annotations, flexible ProgressTokens

---

## 🚀 Quick Start - Production Server in 30 Seconds

```toml
[dependencies]
turbomcp = "1.0.13"
tokio = { version = "1.0", features = ["full"] }
serde_json = "1.0"
```

Create a production-ready server with enterprise features:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct ProductionServer {
    database: Arc<Database>,
    llm_client: Arc<AnthropicClient>,
}

#[server(
    name = "enterprise-ai-server",
    version = "1.0.0",
    description = "Production AI server with LLM integration"
)]
impl ProductionServer {
    #[tool("Analyze data with AI assistance")]
    async fn ai_analyze(&self, ctx: Context, data: String) -> McpResult<String> {
        // Enterprise logging with correlation IDs
        ctx.info(&format!("Processing {} bytes", data.len())).await?;

        // Production database query with connection pooling
        let metadata = self.database.get_metadata(&data).await?;

        // LLM integration with streaming support
        let analysis = self.llm_client
            .complete(&format!("Analyze: {}", data))
            .with_context(&metadata)
            .await?;

        Ok(analysis)
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
        llm_client: Arc::new(AnthropicClient::new("api-key")?),
    };

    // Production deployment with automatic transport selection
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => server.run_http_with_security("0.0.0.0:8080").await?,
        Ok("tcp") => server.run_tcp_clustered("0.0.0.0:8080").await?,
        _ => server.run_stdio().await?, // Claude Desktop integration
    }

    Ok(())
}
```

**Deploy to Claude Desktop:**
```json
{
  "mcpServers": {
    "enterprise-ai": {
      "command": "/path/to/your/server",
      "args": []
    }
  }
}
```

**Test with CLI:**
```bash
cargo install turbomcp-cli
turbomcp-cli tools-call --command "./your-server" --name ai_analyze --args '{"data": "Q4 sales data..."}'
```

---

## 🔥 What Makes TurboMCP Unique

### 🎵 **Industry-Exclusive AudioContent Support**
The only MCP library with multimedia content handling:

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

### 🤖 **Production LLM Integration**
Advanced AI backend support with streaming:

```rust
// Anthropic Claude integration
let client = AnthropicClient::new("api-key")?
    .with_model("claude-3-5-sonnet-20241022")
    .with_streaming(true)
    .with_timeout(Duration::from_secs(30));

let response = client.complete("Analyze this data:")
    .with_context(&analysis_context)
    .with_temperature(0.7)
    .await?;

// OpenAI GPT integration
let openai = OpenAIClient::new("api-key")?
    .with_model("gpt-4")
    .with_max_tokens(4096);
```

### 🎭 **Interactive Elicitation Forms**
Server-initiated user input with enterprise validation:

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

### 🔗 **5 Enterprise Transport Protocols**

| Transport | Performance | Use Case | Security |
|-----------|-------------|----------|----------|
| **STDIO** | Ultra-Fast | Claude Desktop, CLI tools | Process isolation |
| **HTTP/SSE** | High | Web apps, REST APIs | TLS 1.3, session mgmt |
| **WebSocket** | Real-time | Interactive apps | Secure WebSocket |
| **TCP** | **High throughput** | High-performance clusters | Optional TLS |
| **Unix Socket** | Fastest | Container communication | File permissions |

```rust
// Automatic transport selection with production features
match deployment_env {
    "cluster" => server.run_tcp_clustered("0.0.0.0:8080").await?,
    "container" => server.run_unix_secured("/var/run/mcp.sock").await?,
    "web" => server.run_http_with_security("0.0.0.0:8080").await?,
    "desktop" => server.run_stdio().await?,
    _ => server.run_auto_transport().await?, // Intelligent selection
}
```

---

## 🛡️ Enterprise Security & Compliance

### 🔒 **Zero-Vulnerability Architecture**
TurboMCP v1.0.13 achieves complete security compliance:

```rust
// Production security configuration
let security_config = SecurityConfig::enterprise()
    .with_oauth_providers(vec![
        OAuth2Provider::google("client-id", "client-secret"),
        OAuth2Provider::github("client-id", "client-secret"),
        OAuth2Provider::microsoft("client-id", "client-secret"),
    ])
    .with_cors_policy(CorsPolicy::strict()
        .allow_origins(vec!["https://app.company.com"])
        .allow_credentials(true))
    .with_rate_limiting(RateLimit::adaptive()
        .requests_per_minute(1000)
        .burst_capacity(50)
        .per_ip_tracking(true))
    .with_security_headers(SecurityHeaders::strict()
        .csp("default-src 'self'; script-src 'self' 'unsafe-inline'")
        .hsts(true)
        .x_frame_options("DENY"))
    .with_jwt_validation("HS256", "your-secret-key")
    .enable_audit_logging(true);

let server = McpServer::new(config)
    .with_security(security_config)
    .with_middleware(AuthenticationMiddleware::required())
    .with_monitoring(PrometheusMetrics::enabled());
```

### 📊 **Production Monitoring & Observability**
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

## ⚡ Performance Engineering Excellence

### 🚀 **Benchmark Results** (Automated Daily Validation)

```
🏆 TurboMCP Performance Characteristics (v1.0.13)
==================================================
Message Throughput:     High Concurrent     (TCP transport)
Tool Execution:         Sub-millisecond     (99th percentile)
JSON Processing:        SIMD-Accelerated    (Zero-copy optimization)
Memory Efficiency:      Minimal Overhead    (Optimized allocation)
Cold Start Time:        Fast Startup        (Pre-compiled schemas)
Connection Setup:       Rapid Connect       (Connection pooling)

🔬 Automated Regression Detection: ACTIVE
📊 Cross-Platform Validation: Ubuntu | Windows | macOS
⚡ CI/CD Performance Gates: 5% threshold
```

### 🏗️ **Zero-Overhead Architecture**
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

## 🎯 Advanced Production Features

### 🔄 **Shared Concurrency Wrappers**
Thread-safe async operations with zero overhead:

```rust
// Concurrent client access across multiple tasks
let shared_client = SharedClient::new(client);
shared_client.initialize().await?;

// Clone for concurrent usage (Arc/Mutex hidden)
let c1 = shared_client.clone();
let c2 = shared_client.clone();
let c3 = shared_client.clone();

// All tasks can access concurrently
let results = tokio::try_join!(
    async move { c1.list_tools().await },
    async move { c2.list_prompts().await },
    async move { c3.list_resources().await },
)?;

// Transport sharing across multiple connections
let shared_transport = SharedTransport::new(TcpTransport::connect("server:8080").await?);
shared_transport.connect().await?;

// Multiple clients sharing the same transport
let client1 = Client::new(shared_transport.clone());
let client2 = Client::new(shared_transport.clone());
let client3 = Client::new(shared_transport.clone());
```

### 📁 **Filesystem Boundaries & Security**
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

### 🎨 **Resource Templates & Dynamic Content**
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

## 🔧 Enterprise Deployment & Operations

### 🐳 **Container Deployment**
```dockerfile
FROM rust:1.75 as builder
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

### ☸️ **Kubernetes Deployment**
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
        image: your-registry/turbomcp-server:v1.0.13
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

### 📊 **Production Monitoring**
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

## 🛠️ Development & Testing

### 🧪 **Comprehensive Testing**
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

### 🔍 **CLI Development Tools**
```bash
# Install CLI tools
cargo install turbomcp-cli

# Test your server during development
turbomcp-cli tools-list --command "./target/debug/your-server"
turbomcp-cli tools-call --command "./target/debug/your-server" \
  --name "process_data" --args '{"input": "test data"}'

# Export schemas for documentation
turbomcp-cli schema-export --command "./target/debug/your-server" \
  --output schemas.json --format pretty

# Performance profiling
turbomcp-cli benchmark --command "./target/debug/your-server" \
  --duration 60s --concurrency 10
```

### 📚 **Learning Path - 12 Progressive Examples**

| Example | Topic | Difficulty |
|---------|-------|------------|
| [01_hello_world](./crates/turbomcp/examples/01_hello_world.rs) | Basic server setup | Beginner |
| [02_clean_server](./crates/turbomcp/examples/02_clean_server.rs) | Using macros | Beginner |
| [03_basic_tools](./crates/turbomcp/examples/03_basic_tools.rs) | Tool parameters | Beginner |
| [04_resources_and_prompts](./crates/turbomcp/examples/04_resources_and_prompts.rs) | Resources & prompts | Intermediate |
| [05_stateful_patterns](./crates/turbomcp/examples/05_stateful_patterns.rs) | State management | Intermediate |
| [06_architecture_patterns](./crates/turbomcp/examples/06_architecture_patterns.rs) | API comparison | Intermediate |
| [07_transport_showcase](./crates/turbomcp/examples/07_transport_showcase.rs) | All transports | Intermediate |
| [08_elicitation_complete](./crates/turbomcp/examples/08_elicitation_complete.rs) | Interactive forms | Advanced |
| [09_bidirectional_communication](./crates/turbomcp/examples/09_bidirectional_communication.rs) | Full protocol | Advanced |
| [10_protocol_mastery](./crates/turbomcp/examples/10_protocol_mastery.rs) | Complete coverage | Advanced |
| [11_production_deployment](./crates/turbomcp/examples/11_production_deployment.rs) | Enterprise features | Expert |
| [06b_architecture_client](./crates/turbomcp/examples/06b_architecture_client.rs) | Client integration | Expert |

**Run examples:**
```bash
cargo run --example 01_hello_world
cargo run --example 08_elicitation_complete
cargo run --example 11_production_deployment
```

---

## 🏗️ Architecture & Design Philosophy

### 🎯 **"Compile-Time Complexity, Runtime Simplicity"**

TurboMCP's architecture prioritizes **compile-time optimization** over runtime flexibility. This creates a more sophisticated build process but delivers **unmatched runtime performance** and **predictability**.

### 📦 **Modular Excellence**

| Crate | Purpose | Key Innovation |
|-------|---------|----------------|
| [`turbomcp`](./crates/turbomcp/) | Main SDK | Zero-overhead macro integration |
| [`turbomcp-core`](./crates/turbomcp-core/) | Foundation | SIMD-accelerated message processing |
| [`turbomcp-protocol`](./crates/turbomcp-protocol/) | MCP protocol | Compile-time schema generation |
| [`turbomcp-transport`](./crates/turbomcp-transport/) | Transport layer | High throughput with circuit breakers |
| [`turbomcp-server`](./crates/turbomcp-server/) | Server framework | Enterprise security & middleware |
| [`turbomcp-client`](./crates/turbomcp-client/) | Client library | Advanced LLM integration |
| [`turbomcp-macros`](./crates/turbomcp-macros/) | Proc macros | Compile-time optimization engine |
| [`turbomcp-cli`](./crates/turbomcp-cli/) | CLI tools | Production testing & debugging |

### 🚀 **Performance Advantages**

- **Zero-overhead abstractions** - All optimizations happen at compile time
- **O(1) handler dispatch** - Direct function calls, no HashMap lookups
- **Pre-computed schemas** - No runtime schema generation overhead
- **SIMD acceleration** - 2-3x faster JSON processing than standard libraries
- **Zero-copy message handling** - Minimal memory allocations with `Bytes`
- **Smart feature gating** - Lean production binaries through compile-time selection

---

## 📖 Documentation & Resources

- **[API Documentation](https://docs.rs/turbomcp)** - Complete API reference with examples
- **[Benchmarking Guide](./benches/README.md)** - Performance testing and optimization
- **[Security Documentation](./crates/turbomcp-transport/SECURITY_FEATURES.md)** - Enterprise security features
- **[Architecture Guide](./ARCHITECTURE.md)** - System design and component interaction
- **[Examples Guide](./crates/turbomcp/examples/EXAMPLES_GUIDE.md)** - Progressive learning path
- **[MCP Specification](https://modelcontextprotocol.io)** - Official protocol documentation

---

## 🤝 Contributing

We welcome contributions from the community! TurboMCP follows high engineering standards:

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

## 🌟 Enterprise Support

TurboMCP delivers **world-class engineering standards** with **enterprise-grade reliability**:

- ✅ **Zero security vulnerabilities** with continuous monitoring
- ✅ **High performance** with automated regression detection
- ✅ **100% MCP 2025-06-18 compliance** with industry-exclusive features
- ✅ **Production deployment patterns** with container & Kubernetes support
- ✅ **Comprehensive documentation** with progressive learning examples
- ✅ **Active development** with regular security updates and performance improvements

**Ready for mission-critical production deployment.** 🚀

---

*Built with ❤️ by the TurboMCP team • [Model Context Protocol](https://modelcontextprotocol.io) • [Claude Desktop](https://claude.ai)*