# Protocol API Reference

Complete reference for the MCP protocol implementation in `turbomcp-protocol`.

## Core Types

### Request/Response Types

```rust
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<RequestId>,
}

pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
    pub id: RequestId,
}

pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}
```

### Message Types

```rust
pub enum ServerMessage {
    Initialize(InitializeRequest),
    Ping(PingRequest),
    Resource(ResourceRequest),
    Tool(ToolRequest),
    Prompt(PromptRequest),
    Sampling(SamplingRequest),
    Notification(ServerNotification),
}

pub enum ClientMessage {
    InitializeResult(InitializeResult),
    ResourceResult(ResourceResult),
    ToolResult(ToolResult),
    PromptResult(PromptResult),
    SamplingResult(SamplingResult),
    Error(ErrorMessage),
}
```

## Handler Registration

### Tool Registration

Register tool handlers with the protocol layer:

```rust
pub struct ToolRegistry {
    // Map tool names to handlers
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    pub fn register(&mut self, name: String, handler: Arc<dyn ToolHandler>);
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolHandler>>;
    pub fn list(&self) -> Vec<ToolMetadata>;
}
```

### Resource Registration

```rust
pub struct ResourceRegistry {
    handlers: HashMap<String, Arc<dyn ResourceHandler>>,
    templates: HashMap<String, ResourceTemplate>,
}

impl ResourceRegistry {
    pub fn register(&mut self, uri_template: String, handler: Arc<dyn ResourceHandler>);
    pub fn match_uri(&self, uri: &str) -> Option<(Arc<dyn ResourceHandler>, HashMap<String, String>)>;
    pub fn list(&self) -> Vec<ResourceCapability>;
}
```

## Session Management

### Capability Negotiation

```rust
pub struct Capabilities {
    pub sampling: Option<SamplingCapability>,
    pub resources: Option<ResourceCapability>,
    pub tools: Option<ToolCapability>,
    pub prompts: Option<PromptCapability>,
}

pub struct ServerCapabilities {
    pub tools: Option<ToolListChangedCapability>,
    pub resources: Option<ResourceListChangedCapability>,
    pub prompts: Option<PromptListChangedCapability>,
    pub logging: Option<LoggingCapability>,
}
```

### Session State

```rust
pub struct Session {
    pub id: SessionId,
    pub state: SessionState,
    pub client_capabilities: Capabilities,
    pub server_capabilities: ServerCapabilities,
    pub created_at: SystemTime,
    pub last_activity: SystemTime,
}

pub enum SessionState {
    Initializing,
    Active,
    Closing,
    Closed,
}
```

## Error Handling

### Error Types

```rust
pub enum ProtocolError {
    InvalidRequest(String),
    MethodNotFound(String),
    InvalidParams(String),
    InternalError(String),
    ParseError(String),
    ServerError(i32, String),
}

pub enum McpError {
    InvalidInput(String),
    NotFound(String),
    AlreadyExists(String),
    InvalidRequest(String),
    InternalError(String),
    ServiceUnavailable(String),
    PermissionDenied(String),
}
```

## Serialization

### JSON-RPC 2.0 Compliance

All messages use JSON-RPC 2.0 format:

```rust
// Request with ID (expects response)
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {"name": "get_weather", "arguments": {}},
  "id": 1
}

// Response
{
  "jsonrpc": "2.0",
  "result": {"temperature": 72},
  "id": 1
}

// Notification (no response expected)
{
  "jsonrpc": "2.0",
  "method": "resources/updated",
  "params": {}
}
```

### SIMD-Accelerated Processing

Enable with `simd` feature:

```rust
#[cfg(feature = "simd")]
pub fn parse_json_fast(input: &[u8]) -> Result<Value, Error> {
    // Uses simd-json or sonic-rs for 2-3x faster parsing
}
```

## Context API

### RequestInfo

```rust
pub struct RequestInfo {
    pub request_id: String,
    pub correlation_id: String,
    pub timestamp: SystemTime,
    pub client_id: Option<String>,
}
```

### Context

```rust
pub trait Context: Send + Sync {
    fn request_info(&self) -> &RequestInfo;
    fn session(&self) -> &Session;
    fn user(&self) -> Option<&User>;
    fn metadata(&self) -> &HashMap<String, String>;
}
```

## Resource URIs

### URI Templates

Resources use RFC 6570 URI templates:

```rust
// Template: document://{document_id}
// Matches: document://doc123

pub fn match_uri_template(
    template: &str,
    uri: &str,
) -> Option<HashMap<String, String>> {
    // Parses variables from URI
}
```

## Elicitation

### Elicitation Request

```rust
pub struct ElicitationRequest {
    pub request_id: String,
    pub correlation_id: String,
    pub prompt: String,
    pub required: bool,
    pub timeout_ms: Option<u64>,
}

pub struct ElicitationResponse {
    pub request_id: String,
    pub value: Option<String>,
    pub cancelled: bool,
}
```

## Sampling

### Model Sampling API

```rust
pub struct CreateMessageRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub stop_sequences: Option<Vec<String>>,
}

pub struct Message {
    pub role: Role,
    pub content: Content,
}

pub enum Role {
    User,
    Assistant,
}
```

## Validation

### Schema Validation

```rust
pub fn validate_tool_call(
    call: &ToolCall,
    schema: &Schema,
) -> Result<(), ValidationError> {
    // Validates arguments against JSON schema
}
```

### Type Checking

```rust
pub fn check_type_compatibility(
    value: &Value,
    required_type: &JsonSchemaType,
) -> Result<(), TypeError> {
    // Ensures type safety
}
```

## Versioning

### Protocol Version

Current MCP specification: **2025-06-18**

Breaking changes handled through:
- Version negotiation in Initialize
- Feature flags for new capabilities
- Backward compatibility shims

## Performance

### Optimizations

- Zero-copy message handling with `Bytes`
- SIMD-accelerated JSON parsing (optional)
- Connection pooling for transports
- Lazy deserialization

### Benchmarks

- JSON parsing: 0.5-1µs per message
- Tool dispatch: <1ms overhead
- Message routing: <100ns per operation

## Integration Examples

### Using the Protocol Layer

```rust
use turbomcp_protocol::{
    JsonRpcRequest, JsonRpcResponse, RequestId,
};

// Parse incoming JSON-RPC message
let request = serde_json::from_str::<JsonRpcRequest>(json)?;

// Route to handler
match request.method.as_str() {
    "tools/call" => handle_tool_call(request).await,
    "resources/read" => handle_resource_read(request).await,
    _ => Err(MethodNotFound),
}

// Send response
let response = JsonRpcResponse {
    jsonrpc: "2.0".to_string(),
    result: Some(result),
    error: None,
    id: request.id.clone(),
};
```

## See Also

- **[Full Documentation](https://docs.rs/turbomcp-protocol)** on docs.rs
- **[Source Code](../../../crates/turbomcp-protocol/src/lib.rs)**
- **[Protocol Compliance](../architecture/protocol-compliance.md)** guidelines
- **[Examples](../examples/basic.md)** using protocol APIs

