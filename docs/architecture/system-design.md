# System Design & Layered Architecture

Comprehensive technical overview of TurboMCP's modular architecture, design patterns, and engineering decisions.

## Overview

TurboMCP follows a **layered modular architecture** with clear separation of concerns, enabling both rapid prototyping and production-grade optimization. The system is designed around several core principles:

- **Progressive Enhancement** - Start minimal, add features as needed
- **Compile-Time Optimization** - Zero runtime overhead through macros
- **Zero-Copy Performance** - Memory-efficient message processing
- **Type Safety** - Rust's type system prevents entire classes of bugs
- **Composability** - Mix and match transports, middleware, and features

!!! note "Sketches, not source"
    Blocks describing crate internals on this page are simplified sketches,
    marked `rust,ignore`; their type names do not all match the source. In
    particular: `McpError` is a struct with an `ErrorKind` (in `turbomcp-core`),
    not an enum; there is no `HandlerRegistry`, `ContextFactory`, or
    `inventory` registration, because `#[server]` generates one `McpHandler`
    implementation per server type; middleware is the typed
    `turbomcp_server::McpMiddleware` trait; and the only value injected into
    handlers is `&RequestContext`. Blocks that show the public API (the macros,
    the server builder, errors, testing) are real code that compiles against
    the current release.

## Architecture Layers

TurboMCP consists of four distinct architectural layers, each with specific responsibilities:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Layer 4: Developer API                       │
│              turbomcp, turbomcp-macros, turbomcp-cli            │
│         Ergonomic APIs • Macros • CLI Tools • Presets          │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                  Layer 3: Infrastructure                        │
│              turbomcp-server, turbomcp-client                   │
│      Handler Registry • Middleware • Routing • Connection       │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    Layer 2: Transport                           │
│                    turbomcp-transport                           │
│     STDIO • HTTP/SSE • WebSocket • TCP • Unix Sockets          │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    Layer 1: Foundation                          │
│                   turbomcp-protocol                             │
│    JSON-RPC 2.0 • MCP Protocol • Core Types • SIMD • State     │
└─────────────────────────────────────────────────────────────────┘
```

### Layer 1: Foundation (turbomcp-protocol)

**Purpose:** Protocol implementation, core abstractions, and performance-critical types.

**Responsibilities:**
- JSON-RPC 2.0 message format implementation
- MCP protocol version 2025-11-25 compliance
- SIMD-accelerated JSON processing (2-3x faster than serde_json)
- Request/response context management
- Rich error handling with structured context
- Session state management
- Component registry system
- Capability negotiation
- JSON Schema validation
- Zero-copy optimization with `Bytes`

**Key Types:**

```rust,ignore
// Core message types
pub struct JsonRpcRequest { /* ... */ }
pub struct JsonRpcResponse { /* ... */ }
pub struct JsonRpcNotification { /* ... */ }

// MCP protocol messages
pub struct InitializeRequest { /* ... */ }
pub struct InitializeResponse { /* ... */ }
pub struct ToolCallRequest { /* ... */ }
pub struct ResourceReadRequest { /* ... */ }

// Session management
pub struct SessionState {
    client_info: ClientInfo,
    server_capabilities: ServerCapabilities,
    protocol_version: ProtocolVersion,
    // ...
}

// Component registry
pub struct ComponentRegistry {
    tools: HashMap<String, ToolMetadata>,
    resources: HashMap<String, ResourceMetadata>,
    prompts: HashMap<String, PromptMetadata>,
}
```

**Design Decisions:**

1. **SIMD Acceleration** - Optional `simd` feature enables `simd-json` for 2-3x performance improvement
2. **Zero-Copy** - `Bytes` type minimizes allocations during message processing
3. **Error Context** - Rich error types with request correlation and structured context
4. **Thread-Safe** - All state management uses `Arc` and `RwLock` for concurrent access

### Layer 2: Transport (turbomcp-transport)

**Purpose:** Network communication, connection management, and transport protocols.

**Responsibilities:**
- Multi-protocol transport support (STDIO, HTTP, WebSocket, TCP, Unix sockets)
- Connection pooling and lifecycle management
- Security: TLS, authentication, rate limiting, CORS
- Compression and optimization
- Circuit breakers and reliability patterns
- Transport-specific configuration

**Transport Architecture:**

```rust,ignore
// Transport trait abstraction
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, message: Bytes) -> Result<()>;
    async fn receive(&self) -> Result<Bytes>;
    async fn close(&self) -> Result<()>;
}

// Concrete implementations
pub struct StdioTransport { /* ... */ }
pub struct HttpTransport { /* ... */ }
pub struct WebSocketTransport { /* ... */ }
pub struct TcpTransport { /* ... */ }
pub struct UnixSocketTransport { /* ... */ }
```

**Feature-Gated Design:**

Each transport is behind a feature flag to minimize binary size:

```toml
[features]
stdio = []
http = ["axum", "tokio", "hyper"]
websocket = ["tokio-tungstenite", "http"]
tcp = ["tokio"]
unix = ["tokio"]
```

**Security Layers:**

```rust,ignore
// Transport wrapper with security
pub struct SecureTransport<T: Transport> {
    inner: T,
    tls_config: Option<TlsConfig>,
    rate_limiter: RateLimiter,
    circuit_breaker: CircuitBreaker,
}

// Authentication middleware
pub enum AuthMode {
    None,
    ApiKey(String),
    Jwt(JwtValidator),
    OAuth(OAuthConfig),
}
```

### Layer 3: Infrastructure (turbomcp-server, turbomcp-client)

**Purpose:** High-level server/client implementation with routing, middleware, and connection management.

#### Server Architecture (turbomcp-server)

**Handler Registry:**

```rust,ignore
pub struct HandlerRegistry {
    tools: HashMap<String, Box<dyn ToolHandler>>,
    resources: HashMap<String, Box<dyn ResourceHandler>>,
    prompts: HashMap<String, Box<dyn PromptHandler>>,
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn invoke(&self, params: Value, ctx: RequestContext) -> McpResult<Value>;
    fn schema(&self) -> ToolSchema;
}
```

**Request Router:**

```rust,ignore
pub struct RequestRouter {
    registry: Arc<HandlerRegistry>,
    middleware: Vec<Box<dyn Middleware>>,
    context_factory: ContextFactory,
}

impl RequestRouter {
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        // 1. Create request context
        let ctx = self.context_factory.create(request.id);

        // 2. Run middleware chain
        let request = self.run_middleware(request, &ctx).await?;

        // 3. Route to handler
        let response = self.dispatch(request, ctx).await;

        // 4. Run response middleware
        self.run_response_middleware(response).await
    }
}
```

**Middleware Stack:**

```rust,ignore
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn process(
        &self,
        request: JsonRpcRequest,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<JsonRpcResponse>;
}

// Built-in middleware
pub struct LoggingMiddleware { /* ... */ }
pub struct MetricsMiddleware { /* ... */ }
pub struct AuthMiddleware { /* ... */ }
pub struct RateLimitMiddleware { /* ... */ }
pub struct CompressionMiddleware { /* ... */ }
```

#### Client Architecture (turbomcp-client)

**Connection Management:**

```rust,ignore
pub struct Client {
    transport: Arc<dyn Transport>,
    pending_requests: Arc<RwLock<HashMap<RequestId, Sender<JsonRpcResponse>>>>,
    session: Arc<RwLock<Option<SessionState>>>,
    config: ClientConfig,
}

impl Client {
    pub async fn initialize(&self) -> Result<InitializeResponse> {
        // 1. Send initialize request
        let response = self.request(InitializeRequest::new()).await?;

        // 2. Store session state
        let mut session = self.session.write().await;
        *session = Some(SessionState::from(response));

        Ok(response)
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        // 1. Generate request ID
        let request_id = self.generate_id();

        // 2. Create pending response channel
        let (tx, rx) = oneshot::channel();
        self.pending_requests.write().await.insert(request_id, tx);

        // 3. Send request
        self.send(ToolCallRequest { name, args }).await?;

        // 4. Wait for response
        rx.await?
    }
}
```

**Auto-Retry Logic:**

```rust,ignore
pub struct RetryConfig {
    max_attempts: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
    backoff_multiplier: f64,
}

impl Client {
    async fn request_with_retry<T>(&self, request: T) -> Result<T::Response>
    where
        T: Request,
    {
        let mut attempts = 0;
        let mut backoff = self.config.retry.initial_backoff;

        loop {
            match self.request(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) if e.is_retryable() && attempts < self.config.retry.max_attempts => {
                    attempts += 1;
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * self.config.retry.backoff_multiplier as u32)
                        .min(self.config.retry.max_backoff);
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

### Layer 4: Developer API (turbomcp, turbomcp-macros)

**Purpose:** Ergonomic APIs, procedural macros, and zero-boilerplate development.

#### Macro System (turbomcp-macros)

**`#[server]` Macro:**

The macro goes on the server's `impl` block, not on the struct. It generates an
`McpHandler` implementation for the type: server info, the tool, resource, and
prompt catalogues, and a `match`-based dispatcher for each method.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Add two numbers
    #[tool]
    async fn calculate(
        &self,
        #[description("First number")] a: i32,
        #[description("Second number")] b: i32,
    ) -> McpResult<i32> {
        Ok(a + b)
    }
}
```

What it generates, in outline:

```rust,ignore
impl McpHandler for MyServer {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("my-server", "1.0.0")
    }

    fn server_capabilities(&self) -> ServerCapabilities {
        // tools (listChanged), logging; resources/prompts/completions only
        // when the impl block serves them
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![Tool {
            name: "calculate".into(),
            description: Some("Add two numbers".into()),
            input_schema: /* from schemars: a, b integers, required,
                             additionalProperties: false */,
            ..
        }]
    }

    fn call_tool<'a>(&'a self, name: &'a str, args: Value, ctx: &'a RequestContext)
        -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a
    {
        async move {
            match name {
                "calculate" => {
                    // reject unknown arguments, deserialize a and b,
                    // call self.calculate(a, b), convert with IntoToolResult
                }
                _ => Err(McpError::tool_not_found(name)),
            }
        }
    }

    // list_resources, read_resource, list_prompts, get_prompt, and the
    // optional handlers (complete, subscribe, ...) follow the same pattern
}
```

#### High-Level API (turbomcp)

**Fluent Builder Pattern:**

Every `McpHandler` gets `run_*` methods and a `builder()` from blanket
extension traits:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080")) // or stdio/websocket/tcp/unix
        .with_rate_limit(100, Duration::from_secs(1))
        .with_connection_limit(1000)
        .with_protocol(ProtocolConfig::default())
        .serve()
        .await
}
```

**Type-State Builder:**

Capabilities for a hand-written `McpHandler` (or protocol-level code) come from
`turbomcp-protocol`'s type-state builder, where a sub-capability method only
exists once its parent is enabled:

```rust
use turbomcp_protocol::capabilities::builders::ServerCapabilitiesBuilder;

let capabilities = ServerCapabilitiesBuilder::new()
    .enable_tools()
    .enable_tool_list_changed()   // only available after enable_tools()
    .enable_resources()
    .enable_resources_list_changed()
    .build();
```

## Cross-Cutting Concerns

### Dependency Injection

TurboMCP has no dependency-injection container. Request parameters come from
the call's arguments, `&RequestContext` is the one injected parameter, and
shared services live on the server struct:

```rust
use std::sync::Arc;
use turbomcp::prelude::*;

pub struct Settings {
    greeting: String,
}

#[derive(Clone)]
pub struct MyServer {
    settings: Arc<Settings>,
}

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Greet someone
    #[tool]
    async fn my_tool(&self, name: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(format!("{}, {name} (request {})", self.settings.greeting, ctx.request_id()))
    }
}
```

See [Dependency Injection](./dependency-injection.md) for details.

### Context Lifecycle

Every request flows through a well-defined lifecycle:

```
1. Transport receives message
   ↓
2. Deserialize JSON-RPC request
   ↓
3. Create RequestContext
   ↓
4. Run request middleware
   ↓
5. Route to handler
   ↓
6. Extract arguments, pass &RequestContext
   ↓
7. Execute handler
   ↓
8. Run response middleware
   ↓
9. Serialize JSON-RPC response
   ↓
10. Transport sends message
```

See [Context Lifecycle](./context-lifecycle.md) for details.

### Error Handling

**Error Type:**

`McpError` (from `turbomcp-core`, re-exported everywhere) is a struct carrying an
`ErrorKind`, a message, and optional data and context. Constructors pick the
kind, and the kind picks the JSON-RPC code:

```rust
use turbomcp::prelude::*;

fn main() {
    let err = McpError::invalid_params("Name must not be empty");
    assert_eq!(err.jsonrpc_code(), -32602);

    let err = McpError::internal("Database connection failed")
        .with_operation("user_lookup")
        .with_component("auth_service");
    assert_eq!(err.jsonrpc_code(), -32603);

    let err = McpError::resource_not_found("file:///missing.txt");
    assert_eq!(err.jsonrpc_code(), -32002);
}
```

**Error Propagation:**

`McpResult<T>` is `Result<T, McpError>`. Convert other errors at the boundary
with `map_err`, choosing the kind that describes the failure:

```rust
use turbomcp::prelude::*;

fn parse_config(text: &str) -> McpResult<serde_json::Value> {
    serde_json::from_str(text).map_err(|e| McpError::invalid_params(e.to_string()))
}

fn read_config(path: &str) -> McpResult<String> {
    std::fs::read_to_string(path).map_err(|e| McpError::internal(e.to_string()))
}
```

A tool's error reaches the client as a tool execution error (`isError: true`,
the kind in `_meta`); a resource's or prompt's error is a JSON-RPC error.

### Observability

**Structured Logging:**

```rust,ignore
pub struct Logger {
    level: LogLevel,
    fields: HashMap<String, Value>,
    output: Arc<dyn LogOutput>,
}

impl Logger {
    pub async fn info(&self, message: &str) -> Result<()> {
        self.log(LogLevel::Info, message).await
    }

    pub fn with_field(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(key.to_string(), value.into());
        self
    }
}
```

**Metrics Collection:**

```rust,ignore
pub struct Metrics {
    request_counter: Counter,
    request_duration: Histogram,
    active_connections: Gauge,
}

impl Metrics {
    pub fn record_request(&self, method: &str, duration: Duration) {
        self.request_counter
            .with_label_values(&[method])
            .inc();
        self.request_duration
            .with_label_values(&[method])
            .observe(duration.as_secs_f64());
    }
}
```

**Distributed Tracing:**

```rust,ignore
pub struct Tracer {
    provider: Arc<dyn TracerProvider>,
}

impl Tracer {
    pub fn start_span(&self, name: &str) -> Span {
        self.provider
            .tracer("turbomcp")
            .start(name)
    }

    pub async fn trace<F, T>(&self, name: &str, f: F) -> T
    where
        F: Future<Output = T>,
    {
        let span = self.start_span(name);
        let _guard = span.enter();
        f.await
    }
}
```

## Performance Optimizations

### SIMD JSON Processing

When the `simd` feature is enabled:

```rust,ignore
#[cfg(feature = "simd")]
use simd_json::{from_slice, to_vec};

#[cfg(not(feature = "simd"))]
use serde_json::{from_slice, to_vec};

pub fn deserialize_request(bytes: &[u8]) -> Result<JsonRpcRequest> {
    // Automatically uses SIMD when available
    from_slice(bytes)
}
```

**Performance Comparison:**

```
Benchmark: Deserialize 1KB JSON message (1M iterations)
- serde_json:  1,234 ms
- simd-json:     456 ms  (2.7x faster)
- sonic-rs:      389 ms  (3.2x faster)
```

### Zero-Copy Message Processing

```rust,ignore
use bytes::Bytes;

pub struct Message {
    // Zero-copy: shares underlying buffer
    payload: Bytes,
}

impl Message {
    pub fn parse(&self) -> Result<JsonRpcRequest> {
        // No allocation: borrows from Bytes
        simd_json::from_slice(&self.payload)
    }

    pub fn clone(&self) -> Self {
        // Cheap: just increments Arc refcount
        Self {
            payload: self.payload.clone(),
        }
    }
}
```

### Connection Pooling

```rust,ignore
pub struct ConnectionPool {
    connections: Vec<Arc<Connection>>,
    available: Arc<Mutex<VecDeque<usize>>>,
    max_size: usize,
}

impl ConnectionPool {
    pub async fn acquire(&self) -> Result<PooledConnection> {
        let idx = self.available
            .lock()
            .await
            .pop_front()
            .ok_or(PoolExhausted)?;

        Ok(PooledConnection {
            conn: self.connections[idx].clone(),
            pool: self.clone(),
            idx,
        })
    }

    pub async fn release(&self, idx: usize) {
        self.available.lock().await.push_back(idx);
    }
}
```

### Arc-Cloning Pattern

Following Axum/Tower conventions, `McpHandler` requires `Clone`, and each
transport clones the handler per connection or request. Keep that cheap by
holding state behind an `Arc`:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
pub struct Counter {
    // Cloning the server only increments this refcount
    count: Arc<RwLock<u64>>,
}

#[server(name = "counter", version = "1.0.0")]
impl Counter {
    /// Increment and return the counter
    #[tool]
    async fn increment(&self) -> u64 {
        let mut count = self.count.write().await;
        *count += 1;
        *count
    }
}

#[tokio::main]
async fn main() {
    let server = Counter::default();
    let server1 = server.clone();
    let server2 = server.clone();

    // Both clones share the same counter
    let a = tokio::spawn(async move { server1.increment().await });
    let b = tokio::spawn(async move { server2.increment().await });
    let _ = tokio::join!(a, b);
    assert_eq!(server.increment().await, 3);
}
```

## Design Patterns

### Type-State Pattern

Enforce correctness at compile time:

```rust,ignore
pub struct ServerBuilder<State> {
    _state: PhantomData<State>,
    info: Option<ServerInfo>,
    capabilities: Option<ServerCapabilities>,
}

pub struct Uninitialized;
pub struct WithInfo;
pub struct WithCapabilities;

impl ServerBuilder<Uninitialized> {
    pub fn new() -> Self { /* ... */ }

    pub fn with_info(self, info: ServerInfo) -> ServerBuilder<WithInfo> {
        // State transition
    }
}

impl ServerBuilder<WithInfo> {
    pub fn with_capabilities(
        self,
        caps: ServerCapabilities,
    ) -> ServerBuilder<WithCapabilities> {
        // State transition
    }
}

impl ServerBuilder<WithCapabilities> {
    // Only available in final state
    pub fn build(self) -> McpServer {
        // ...
    }
}
```

### Builder Pattern

Fluent API for configuration, shown in [High-Level API](#high-level-api-turbomcp)
above: `ServerConfig::builder()` for the configuration and
`handler.builder()` for the transport and limits.

### Newtype Pattern

Type safety for primitive values:

```rust,ignore
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CorrelationId(String);

// Prevents mixing up IDs
fn process(request_id: RequestId, correlation_id: CorrelationId) {
    // Compiler enforces correct usage
}
```

### Trait Objects for Extensibility

Middleware implements the typed `McpMiddleware` trait, overriding only the hooks
it needs (each has a pass-through default), and `MiddlewareStack` composes it
around any handler:

```rust
use std::future::Future;
use std::pin::Pin;
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, MiddlewareStack, Next};

pub struct CustomMiddleware;

impl McpMiddleware for CustomMiddleware {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            // Custom logic
            next.call_tool(name, args, ctx).await
        })
    }
}

#[derive(Clone)]
pub struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

fn stack() -> MiddlewareStack<MyServer> {
    MiddlewareStack::new(MyServer).with_middleware(CustomMiddleware)
}
```

## Security Architecture

### Input Validation

**Identifier Validation:**

```rust,ignore
use syn::Ident;

pub fn validate_identifier(name: &str) -> Result<()> {
    // Use syn crate for robust Rust identifier validation
    syn::parse_str::<Ident>(name)
        .map(|_| ())
        .map_err(|_| McpError::InvalidParams("Invalid identifier".into()))
}
```

**Path Traversal Prevention:**

```rust,ignore
use std::path::{Path, PathBuf};

pub fn validate_path(base: &Path, requested: &Path) -> Result<PathBuf> {
    let canonical = base.join(requested).canonicalize()?;

    if !canonical.starts_with(base) {
        return Err(McpError::InvalidParams("Path traversal detected".into()));
    }

    Ok(canonical)
}
```

**SSRF Prevention:**

```rust,ignore
use ipnetwork::IpNetwork;

pub fn validate_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url)?;

    // Block private IP ranges
    if let Some(host) = parsed.host() {
        match host {
            url::Host::Ipv4(ip) => {
                if is_private_ipv4(ip) {
                    return Err(McpError::InvalidParams("Private IP blocked".into()));
                }
            }
            url::Host::Ipv6(ip) => {
                if is_private_ipv6(ip) {
                    return Err(McpError::InvalidParams("Private IP blocked".into()));
                }
            }
            _ => {}
        }
    }

    Ok(())
}
```

### Rate Limiting

The HTTP transport has a built-in per-client token-bucket limiter, configured
with `builder().with_rate_limit(max_requests, window)` or
`ServerConfig::builder().rate_limit(RateLimitConfig::new(...))`. The per-client
key is the client IP; `X-Forwarded-For` and similar headers are honoured only
from `OriginValidationConfig::trusted_proxies`.

### Authentication

The Streamable HTTP transport implements MCP authorization:
`ServerConfig::builder().authorization(HttpAuthorization::new(resource, auth_server, validator))`
publishes RFC 9728 Protected Resource Metadata, answers requests without a valid
bearer token `401` with a `WWW-Authenticate` challenge, and puts the validated
`Principal` on the request context. `turbomcp_auth::server::JwtBearerValidator`
is the JWT validator. See [Authentication](../guide/authentication.md).

## Testing Architecture

### Unit Tests

Handler methods stay ordinary methods, and `McpTestClient` runs calls through
MCP dispatch without a transport:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Echo the argument
    #[tool]
    async fn echo(&self, arg: String) -> String {
        arg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_invocation() {
        // Directly
        assert_eq!(MyServer.echo("value".into()).await, "value");

        // Through dispatch, argument validation included
        let client = McpTestClient::new(MyServer);
        let result = client
            .call_tool("echo", serde_json::json!({"arg": "value"}))
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("value"));
    }
}
```

### Integration Tests

`handle_request` runs a raw JSON-RPC request through the router the
transports use:

```rust
use serde_json::json;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// A test tool
    #[tool]
    async fn test(&self) -> String {
        "ok".to_string()
    }
}

#[tokio::test]
async fn test_full_request_flow() {
    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": { "name": "test", "arguments": {} },
        "id": 1
    });

    let response = MyServer
        .handle_request(request, RequestContext::new())
        .await
        .unwrap();
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["content"][0]["text"], "ok");
}
```

### Property-Based Testing

Sketch (needs `proptest`, and `validate_identifier` stands for your own code):

```rust,ignore
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_identifier_validation(s in "\\PC*") {
        // Property: valid identifiers never contain invalid characters
        let result = validate_identifier(&s);
        if result.is_ok() {
            assert!(s.chars().all(|c| c.is_alphanumeric() || c == '_'));
        }
    }
}
```

### Fuzzing

`crates/turbomcp-protocol/fuzz` has `cargo-fuzz` targets for JSON-RPC parsing,
message validation, capability parsing, and tool deserialization; run them with
`cargo fuzz run <target>` from that directory.

## Related Documentation

- [Context Lifecycle](./context-lifecycle.md) - Request flow and context management
- [Dependency Injection](./dependency-injection.md) - Handler parameters and shared state
- [Protocol Compliance](./protocol-compliance.md) - MCP protocol compliance
- [ARCHITECTURE.md](../../ARCHITECTURE.md) - High-level architecture overview
- [Advanced Patterns](../guide/advanced-patterns.md) - Implementation patterns
- [Observability](../guide/observability.md) - Logging, metrics, and tracing
