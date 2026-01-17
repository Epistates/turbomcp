# TurboMCP v3 Architecture Design

> **Status**: In Progress
> **Reference Branch**: `gemini-v3-attempt-reference` (contains incomplete prior attempt)
> **Target**: Clean, SOTA MCP SDK implementation

## Executive Summary

TurboMCP v3 is a major architectural redesign focused on:
1. **Single source of truth** for all MCP types (`turbomcp-types`)
2. **Transport-agnostic handler trait** (`McpHandler`)
3. **Zero-boilerplate server creation** via proc macros
4. **no_std foundation** for WASM/edge deployment

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Application Layer                              │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐               │
│   │   Your MCP   │  │    Demo     │  │   Examples   │               │
│   │   Server     │  │   Server    │  │              │               │
│   └──────────────┘  └──────────────┘  └──────────────┘               │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                        SDK Layer (turbomcp)                           │
│   • High-level API                                                    │
│   • Re-exports everything users need                                  │
│   • Transport runners (run_stdio, run_http)                          │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                        Macro Layer (turbomcp-macros)                  │
│   • #[server] - generates McpHandler implementation                  │
│   • #[tool] - marks tool handlers, generates JSON schema              │
│   • #[resource] - marks resource handlers                            │
│   • #[prompt] - marks prompt handlers                                │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     Runtime Layer (turbomcp-server)                   │
│   • JSON-RPC routing                                                 │
│   • Transport implementations (STDIO, HTTP, WebSocket)               │
│   • Server configuration                                             │
│   • Connection management                                            │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     Protocol Layer (turbomcp-protocol)                │
│   • Advanced protocol features                                       │
│   • Session management                                               │
│   • Capability negotiation                                           │
│   • Validation                                                       │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     Core Layer (turbomcp-core)                        │
│   • McpHandler trait definition                                      │
│   • RequestContext (transport-agnostic)                              │
│   • IntoToolResult, IntoResourceResult traits                        │
│   • no_std compatible                                                │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     Types Layer (turbomcp-types)                      │
│   • ALL MCP type definitions                                         │
│   • Content types (TextContent, ImageContent, etc.)                  │
│   • Definition types (Tool, Resource, Prompt, ServerInfo)            │
│   • Result types (ToolResult, ResourceResult, PromptResult)          │
│   • Error types (McpError)                                           │
│   • Single source of truth                                           │
└──────────────────────────────────────────────────────────────────────┘
```

## Core Design Principles

### 1. Single Source of Truth (`turbomcp-types`)

All MCP types are defined ONCE in `turbomcp-types`. Other crates re-export, never duplicate.

```rust
// turbomcp-types/src/lib.rs
pub mod content;      // TextContent, ImageContent, AudioContent, etc.
pub mod definitions;  // Tool, Resource, Prompt, ServerInfo
pub mod results;      // ToolResult, ResourceResult, PromptResult
pub mod error;        // McpError, ErrorKind
pub mod traits;       // IntoToolResult, IntoResourceResult
```

### 2. Transport-Agnostic Handler (`McpHandler`)

The `McpHandler` trait defines the interface for ALL MCP operations:

```rust
// turbomcp-core/src/handler.rs
pub trait McpHandler: Clone + Send + Sync + 'static {
    /// Returns server information for initialization
    fn server_info(&self) -> ServerInfo;

    /// Lists all available tools
    fn list_tools(&self) -> Vec<Tool>;

    /// Lists all available resources
    fn list_resources(&self) -> Vec<Resource>;

    /// Lists all available prompts
    fn list_prompts(&self) -> Vec<Prompt>;

    /// Calls a tool by name
    fn call_tool(
        &self,
        name: &str,
        args: Value,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send;

    /// Reads a resource by URI
    fn read_resource(
        &self,
        uri: &str,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + Send;

    /// Gets a prompt by name
    fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + Send;
}
```

### 3. Minimal Request Context

```rust
// turbomcp-core/src/context.rs
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub request_id: String,
    pub transport: TransportType,
    pub headers: Option<BTreeMap<String, String>>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    Stdio,
    Http,
    WebSocket,
    Tcp,
    Unix,
    Wasm,
}
```

### 4. Zero-Boilerplate Macros

```rust
// User code - this is ALL you need
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Adds two numbers together
    #[tool]
    async fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }

    /// Gets the current time
    #[tool]
    async fn get_time(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }

    /// Reads a file resource
    #[resource("file://{path}")]
    async fn read_file(&self, path: String) -> String {
        std::fs::read_to_string(&path).unwrap_or_default()
    }
}

#[tokio::main]
async fn main() {
    MyServer.run_stdio().await;
}
```

The `#[server]` macro generates:
- `impl McpHandler for MyServer { ... }`
- JSON schema for all tool parameters
- Tool/resource/prompt routing logic

## Implementation Plan

### Phase 1: Foundation (turbomcp-types, turbomcp-core)

1. **Audit turbomcp-types** - ensure all MCP 2025-11-25 types are defined
2. **Create McpHandler trait** in turbomcp-core
3. **Create RequestContext** in turbomcp-core
4. **Verify no_std compatibility**

### Phase 2: Macros (turbomcp-macros/v3)

1. **#[tool] macro** - extract tool info, generate schema
2. **#[resource] macro** - extract URI templates
3. **#[prompt] macro** - extract prompt arguments
4. **#[server] macro** - generate McpHandler impl

### Phase 3: Runtime (turbomcp-server/v3)

1. **JSON-RPC router** - route requests to McpHandler methods
2. **STDIO transport** - read/write JSON-RPC over stdin/stdout
3. **HTTP transport** - Axum-based HTTP/SSE server
4. **WebSocket transport** - bidirectional support

### Phase 4: Integration

1. **Update turbomcp** to re-export everything
2. **Update examples** to use new API
3. **Migration guide** for v2 users
4. **Performance benchmarks**

## Type Definitions (turbomcp-types)

### Content Types

```rust
// Already well-defined in turbomcp-types/src/content.rs
pub enum Content {
    Text(TextContent),
    Image(ImageContent),
    Audio(AudioContent),
    Resource(EmbeddedResource),
}

pub struct TextContent {
    pub text: String,
    pub annotations: Option<Annotations>,
}

pub struct ImageContent {
    pub data: String,  // base64
    pub mime_type: String,
    pub annotations: Option<Annotations>,
}
```

### Definition Types

```rust
// turbomcp-types/src/definitions.rs
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: ToolInputSchema,
    pub annotations: Option<ToolAnnotations>,
}

pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

pub struct Prompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}

pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}
```

### Result Types

```rust
// turbomcp-types/src/results.rs
pub struct ToolResult {
    pub content: Vec<Content>,
    pub is_error: bool,
}

pub struct ResourceResult {
    pub contents: Vec<ResourceContent>,
}

pub struct PromptResult {
    pub description: Option<String>,
    pub messages: Vec<Message>,
}
```

### Error Type

```rust
// turbomcp-types/src/error.rs
#[derive(Debug, Clone)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

impl McpError {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    pub const TOOL_NOT_FOUND: i32 = -32001;
    // ... etc
}
```

## JSON-RPC Router

The router dispatches JSON-RPC requests to McpHandler methods:

```rust
// turbomcp-server/src/v3/router.rs
pub async fn route_request<H: McpHandler>(
    handler: &H,
    request: JsonRpcRequest,
    ctx: &RequestContext,
) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(handler, request.params),
        "tools/list" => handle_tools_list(handler),
        "tools/call" => handle_tools_call(handler, request.params, ctx).await,
        "resources/list" => handle_resources_list(handler),
        "resources/read" => handle_resources_read(handler, request.params, ctx).await,
        "prompts/list" => handle_prompts_list(handler),
        "prompts/get" => handle_prompts_get(handler, request.params, ctx).await,
        "ping" => handle_ping(),
        _ => JsonRpcResponse::error(McpError::method_not_found(&request.method)),
    }
}
```

## Transport Implementations

### STDIO

```rust
// turbomcp-server/src/v3/transports/stdio.rs
pub async fn run_stdio<H: McpHandler>(handler: H) {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let request: JsonRpcRequest = serde_json::from_str(&line)?;
        let ctx = RequestContext::new(request.id.clone(), TransportType::Stdio);
        let response = route_request(&handler, request, &ctx).await;
        let json = serde_json::to_string(&response)?;
        stdout.write_all(json.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
}
```

### HTTP (Axum)

```rust
// turbomcp-server/src/v3/transports/http.rs
pub async fn run_http<H: McpHandler>(handler: H, addr: SocketAddr) {
    let app = Router::new()
        .route("/mcp", post(handle_mcp_request::<H>))
        .with_state(handler);

    axum::serve(TcpListener::bind(addr).await?, app).await?;
}

async fn handle_mcp_request<H: McpHandler>(
    State(handler): State<H>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let ctx = RequestContext::from_http(request.id.clone(), &headers);
    Json(route_request(&handler, request, &ctx).await)
}
```

## Macro Code Generation

The `#[server]` macro transforms:

```rust
#[server]
impl MyServer {
    #[tool]
    async fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }
}
```

Into:

```rust
impl MyServer {
    async fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }
}

impl McpHandler for MyServer {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("MyServer", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            Tool {
                name: "add".to_string(),
                description: None,
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some(json!({
                        "a": { "type": "integer" },
                        "b": { "type": "integer" }
                    })),
                    required: Some(vec!["a".to_string(), "b".to_string()]),
                    additional_properties: Some(false),
                },
                ..Default::default()
            }
        ]
    }

    fn list_resources(&self) -> Vec<Resource> { vec![] }
    fn list_prompts(&self) -> Vec<Prompt> { vec![] }

    fn call_tool(
        &self,
        name: &str,
        args: Value,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send {
        let handler = self.clone();
        let name = name.to_string();
        async move {
            match name.as_str() {
                "add" => {
                    let a: i32 = serde_json::from_value(args["a"].clone())?;
                    let b: i32 = serde_json::from_value(args["b"].clone())?;
                    let result = handler.add(a, b).await;
                    Ok(result.into_tool_result())
                }
                _ => Err(McpError::tool_not_found(&name))
            }
        }
    }

    // ... read_resource, get_prompt implementations
}
```

## Migration from v2

### Before (v2)

```rust
use turbomcp::prelude::*;
use turbomcp_macros::server;

#[server]
impl MyServer {
    #[tool]
    async fn my_tool(&self, ctx: Context, input: String) -> Result<String, Error> {
        Ok(input)
    }
}
```

### After (v3)

```rust
use turbomcp::prelude::*;

#[server]
impl MyServer {
    #[tool]
    async fn my_tool(&self, input: String) -> String {
        input
    }
}
```

Key changes:
1. Context is optional (injected if parameter named `ctx` exists)
2. Return types are automatically converted via `IntoToolResult`
3. No need for explicit `Result` wrapping

## Lessons from Prior Attempt

The `gemini-v3-attempt-reference` branch showed:

### Good Ideas (Keep)
- Unified types in `turbomcp-types`
- `McpHandler` trait design
- `RequestContext` simplicity
- Macro code generation approach

### Mistakes (Avoid)
- Deleted files without updating consumers
- Left compilation errors
- Incomplete type migrations
- Missing integration between layers

## Success Criteria

v3 is complete when:
- [ ] `cargo check --workspace` passes with zero errors
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace` passes
- [ ] All examples work
- [ ] Demo server runs on STDIO
- [ ] Demo server runs on HTTP
- [ ] Documentation is complete

## File Structure

```
crates/
├── turbomcp-types/          # All MCP type definitions
│   └── src/
│       ├── lib.rs
│       ├── content.rs       # Content types
│       ├── definitions.rs   # Tool, Resource, Prompt, ServerInfo
│       ├── results.rs       # ToolResult, ResourceResult, PromptResult
│       ├── error.rs         # McpError
│       └── traits.rs        # IntoToolResult, etc.
│
├── turbomcp-core/           # no_std foundation
│   └── src/
│       ├── lib.rs
│       ├── handler.rs       # McpHandler trait
│       ├── context.rs       # RequestContext
│       └── response.rs      # IntoToolResponse helpers
│
├── turbomcp-macros/         # Procedural macros
│   └── src/
│       ├── lib.rs
│       └── v3/
│           ├── mod.rs
│           ├── server.rs    # #[server] macro
│           ├── tool.rs      # #[tool] macro
│           ├── resource.rs  # #[resource] macro
│           └── prompt.rs    # #[prompt] macro
│
├── turbomcp-server/         # Server runtime
│   └── src/
│       ├── lib.rs
│       └── v3/
│           ├── mod.rs
│           ├── router.rs    # JSON-RPC routing
│           ├── config.rs    # Server configuration
│           └── transports/
│               ├── mod.rs
│               ├── stdio.rs
│               └── http.rs
│
├── turbomcp-protocol/       # Advanced protocol (unchanged for now)
│
└── turbomcp/                # Main SDK crate
    └── src/
        └── lib.rs           # Re-exports everything
```

---

*Document created: 2026-01-15*
*Author: Claude Opus 4.5 assisting with TurboMCP v3 development*
