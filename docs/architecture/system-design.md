# System Design & Layered Architecture

Comprehensive technical overview of TurboMCP's modular architecture, design patterns, and engineering decisions.

## Overview

TurboMCP follows a **layered modular architecture** with clear separation of concerns, enabling both rapid prototyping and production-grade optimization. The system is designed around several core principles:

- **Progressive Enhancement** - Start minimal, add features as needed
- **Compile-Time Optimization** - Zero runtime overhead through macros
- **Zero-Copy Performance** - Memory-efficient message processing
- **Type Safety** - Rust's type system prevents entire classes of bugs
- **Composability** - Mix and match transports, middleware, and features

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
- MCP protocol version 2025-06-18 compliance
- SIMD-accelerated JSON processing (2-3x faster than serde_json)
- Request/response context management
- Rich error handling with structured context
- Session state management
- Component registry system
- Capability negotiation
- JSON Schema validation
- Zero-copy optimization with `Bytes`

**Key Types:**

```rust
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

```rust
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

```rust
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

```rust
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

```rust
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

```rust
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

```rust
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

```rust
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

```rust
// User writes this:
#[server]
pub struct MyServer;

// Macro generates:
impl MyServer {
    pub fn new() -> McpServer<Self> {
        McpServer::new()
            .with_info(/* ... */)
            .with_capabilities(/* ... */)
    }
}

impl Default for MyServer {
    fn default() -> Self {
        Self
    }
}
```

**`#[tool]` Macro:**

```rust
// User writes this:
#[tool]
pub async fn calculate(
    #[description("First number")] a: i32,
    #[description("Second number")] b: i32,
) -> McpResult<i32> {
    Ok(a + b)
}

// Macro generates:
pub struct CalculateTool;

#[async_trait]
impl ToolHandler for CalculateTool {
    async fn invoke(&self, params: Value, ctx: RequestContext) -> McpResult<Value> {
        // 1. Deserialize parameters
        let a: i32 = params.get("a").ok_or(...)?.as_i64()? as i32;
        let b: i32 = params.get("b").ok_or(...)?.as_i64()? as i32;

        // 2. Call function
        let result = calculate(a, b).await?;

        // 3. Serialize result
        Ok(serde_json::to_value(result)?)
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "calculate".to_string(),
            description: None,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "a": {
                        "type": "integer",
                        "description": "First number"
                    },
                    "b": {
                        "type": "integer",
                        "description": "Second number"
                    }
                },
                "required": ["a", "b"]
            }),
        }
    }
}

// Register in inventory
inventory::submit! {
    ToolRegistration::new("calculate", Box::new(CalculateTool))
}
```

#### High-Level API (turbomcp)

**Fluent Builder Pattern:**

```rust
let server = McpServer::new()
    .with_info(ServerInfo {
        name: "my-server".to_string(),
        version: "1.0.0".to_string(),
    })
    .with_capabilities(|caps| {
        caps.with_tools()
            .with_resources()
            .with_prompts()
            .tool_list_changed(true)  // Only available when tools enabled
            .resource_list_changed(true)  // Only available when resources enabled
    })
    .with_logging(LogConfig::default())
    .with_metrics(MetricsConfig::default())
    .stdio()  // Choose transport
    .run()
    .await?;
```

**Type-State Builder:**

```rust
// Capability builder uses type-state pattern
pub struct CapabilityBuilder<Tools, Resources, Prompts> {
    _tools: PhantomData<Tools>,
    _resources: PhantomData<Resources>,
    _prompts: PhantomData<Prompts>,
    // ...
}

// These methods only exist when specific capabilities are enabled
impl<T, R, P> CapabilityBuilder<T, R, P> {
    pub fn with_tools(self) -> CapabilityBuilder<Enabled, R, P> { /* ... */ }
    pub fn with_resources(self) -> CapabilityBuilder<T, Enabled, P> { /* ... */ }
    pub fn with_prompts(self) -> CapabilityBuilder<T, R, Enabled> { /* ... */ }
}

impl<R, P> CapabilityBuilder<Enabled, R, P> {
    // Only available when tools are enabled
    pub fn tool_list_changed(self, value: bool) -> Self { /* ... */ }
}
```

## Cross-Cutting Concerns

### Dependency Injection

TurboMCP provides compile-time dependency injection for handlers:

```rust
#[tool]
pub async fn my_tool(
    // Request parameters
    name: String,

    // Injected dependencies
    ctx: Context,
    logger: Logger,
    cache: Cache,
    db: Database,
    client: HttpClient,
) -> McpResult<String> {
    // Implementation
}
```

**Injection Resolution:**

```rust
pub struct ContextFactory {
    providers: HashMap<TypeId, Box<dyn Provider>>,
}

impl ContextFactory {
    pub fn resolve<T: 'static>(&self) -> Option<T> {
        self.providers
            .get(&TypeId::of::<T>())
            .and_then(|p| p.downcast_ref::<T>())
            .cloned()
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
6. Inject dependencies
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

**Error Type Hierarchy:**

```rust
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Transport error: {0}")]
    TransportError(#[from] TransportError),
}

// JSON-RPC error codes
impl McpError {
    pub fn code(&self) -> i32 {
        match self {
            McpError::InvalidRequest(_) => -32600,
            McpError::MethodNotFound(_) => -32601,
            McpError::InvalidParams(_) => -32602,
            McpError::InternalError(_) => -32603,
            McpError::TransportError(_) => -32000,
        }
    }
}
```

**Error Propagation:**

```rust
pub type McpResult<T> = Result<T, McpError>;

// Automatic conversion from common error types
impl From<serde_json::Error> for McpError {
    fn from(e: serde_json::Error) -> Self {
        McpError::InvalidParams(e.to_string())
    }
}

impl From<std::io::Error> for McpError {
    fn from(e: std::io::Error) -> Self {
        McpError::InternalError(e.to_string())
    }
}
```

### Observability

**Structured Logging:**

```rust
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

```rust
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

```rust
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

```rust
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

```rust
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

```rust
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

Following Axum/Tower conventions:

```rust
#[derive(Clone)]
pub struct McpServer<S> {
    inner: Arc<ServerInner<S>>,
}

impl<S> McpServer<S> {
    pub fn clone(&self) -> Self {
        // Cheap: just Arc increment
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

// Can be passed to multiple tasks
let server = McpServer::new();
let server1 = server.clone();
let server2 = server.clone();

tokio::spawn(async move {
    server1.handle_request(req1).await;
});

tokio::spawn(async move {
    server2.handle_request(req2).await;
});
```

## Design Patterns

### Type-State Pattern

Enforce correctness at compile time:

```rust
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

Fluent API for configuration:

```rust
let server = McpServer::new()
    .with_info(/* ... */)
    .with_capabilities(|caps| {
        caps.with_tools()
            .with_resources()
    })
    .with_middleware(LoggingMiddleware::new())
    .with_middleware(MetricsMiddleware::new())
    .stdio()
    .run()
    .await?;
```

### Newtype Pattern

Type safety for primitive values:

```rust
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

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn process(
        &self,
        request: JsonRpcRequest,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<JsonRpcResponse>;
}

// Users can implement custom middleware
pub struct CustomMiddleware;

#[async_trait]
impl Middleware for CustomMiddleware {
    async fn process(&self, req: JsonRpcRequest, ctx: &RequestContext, next: Next<'_>) -> Result<JsonRpcResponse> {
        // Custom logic
        next.run(req, ctx).await
    }
}
```

## Security Architecture

### Input Validation

**Identifier Validation:**

```rust
use syn::Ident;

pub fn validate_identifier(name: &str) -> Result<()> {
    // Use syn crate for robust Rust identifier validation
    syn::parse_str::<Ident>(name)
        .map(|_| ())
        .map_err(|_| McpError::InvalidParams("Invalid identifier".into()))
}
```

**Path Traversal Prevention:**

```rust
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

```rust
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

```rust
use governor::{Quota, RateLimiter as GovernorRateLimiter};

pub struct RateLimiter {
    limiter: GovernorRateLimiter<String, DefaultHasher>,
}

impl RateLimiter {
    pub fn new(requests_per_second: u32) -> Self {
        let quota = Quota::per_second(requests_per_second);
        Self {
            limiter: GovernorRateLimiter::keyed(quota),
        }
    }

    pub async fn check(&self, key: &str) -> Result<()> {
        self.limiter
            .check_key(key)
            .map(|_| ())
            .map_err(|_| McpError::RateLimitExceeded)
    }
}
```

### Authentication

```rust
pub enum AuthProvider {
    ApiKey(ApiKeyAuth),
    Jwt(JwtAuth),
    OAuth(OAuthProvider),
}

impl AuthProvider {
    pub async fn authenticate(&self, headers: &HeaderMap) -> Result<Claims> {
        match self {
            AuthProvider::ApiKey(auth) => auth.validate(headers),
            AuthProvider::Jwt(auth) => auth.validate(headers),
            AuthProvider::OAuth(auth) => auth.validate(headers).await,
        }
    }
}
```

## Testing Architecture

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_invocation() {
        let handler = MyToolHandler;
        let params = json!({"arg": "value"});
        let ctx = RequestContext::test();

        let result = handler.invoke(params, ctx).await;
        assert!(result.is_ok());
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_full_request_flow() {
    let server = McpServer::new()
        .with_test_config()
        .stdio()
        .build();

    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/call".to_string(),
        params: json!({"name": "test", "arguments": {}}),
        id: Some(json!(1)),
    };

    let response = server.handle_request(request).await.unwrap();
    assert_eq!(response.id, Some(json!(1)));
}
```

### Property-Based Testing

```rust
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

```rust
#[cfg(fuzzing)]
pub fn fuzz_json_rpc_parsing(data: &[u8]) {
    let _ = serde_json::from_slice::<JsonRpcRequest>(data);
}
```

## Related Documentation

- [Context Lifecycle](./context-lifecycle.md) - Request flow and context management
- [Dependency Injection](./dependency-injection.md) - DI system implementation
- [Protocol Compliance](./protocol-compliance.md) - MCP protocol compliance
- [ARCHITECTURE.md](../../ARCHITECTURE.md) - High-level architecture overview
- [Advanced Patterns](../guide/advanced-patterns.md) - Implementation patterns
- [Observability](../guide/observability.md) - Logging, metrics, and tracing
