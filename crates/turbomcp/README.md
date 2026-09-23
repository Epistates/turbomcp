# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../../LICENSE)
[![Tests](https://github.com/Epistates/turbomcp/actions/workflows/test.yml/badge.svg)](https://github.com/Epistates/turbomcp/actions/workflows/test.yml)

Rust SDK for the Model Context Protocol (MCP) with comprehensive specification support and performance optimizations.

## Quick Navigation

**Jump to section:**
[Overview](#overview) | [Quick Start](#quick-start) | [Core Concepts](#core-concepts) | [Advanced Features](#mcp-2025-11-25-enhanced-features) | [Security](#security-features) | [Performance](#performance) | [Deployment](#deployment--operations) | [Examples](#examples)

## Overview

`turbomcp` is a Rust framework for implementing the Model Context Protocol. It provides tools, servers, clients, and transport layers with MCP specification compliance, security features, and performance optimizations.

### Security Features
- Zero known vulnerabilities - Security audit with `cargo-deny` policy
- Dependency security - Eliminated RSA and paste crate vulnerabilities
- MIT-compatible dependencies - Permissive license enforcement
- Security hardening - Dependency optimization for security

### Performance Monitoring
- Benchmarking infrastructure - Automated regression detection
- Cross-platform testing - Ubuntu, Windows, macOS CI validation
- CI/CD integration - GitHub Actions with performance tracking

## Key Features

### Performance Features
- Optimized JSON processing - Optional SIMD acceleration with fast libraries
- Efficient message handling - Minimal memory allocations with zero-copy patterns
- Connection management - Connection pooling and reuse strategies
- Request routing - Efficient handler lookup with parameter injection

### Developer Experience
- Procedural macros - `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]`
- Type-state capability builders - Compile-time validated capability configuration
- Automatic schema generation - JSON schemas from Rust types
- Type-safe parameters - Compile-time validation and conversion
- Context injection - Request context available in handler signatures
- Builder patterns for user input and message handling
- Context API - Access to user information, authentication, and request metadata

### Security Features
- OAuth 2.0 integration - Google, GitHub, Microsoft provider support
- PKCE security - Proof Key for Code Exchange implementation
- CORS protection - Cross-origin resource sharing policies
- Rate limiting - Token bucket algorithm with burst capacity
- Security headers - CSP, HSTS, X-Frame-Options configuration

### Multi-Transport Support
- STDIO - Command-line integration with protocol compliance
- **Streamable HTTP** - MCP HTTP transport with session management and SSE support
- **WebSocket** - Real-time bidirectional communication with connection lifecycle management
- **TCP** - Direct socket connections with connection pooling
- **Unix Sockets** - Local inter-process communication with file permissions

All transport protocols provide MCP protocol compliance with bidirectional communication, automatic reconnection, and session management.

> **⚠️ STDIO Transport Output Constraint** ⚠️
>
> When using STDIO transport, **ALL application output must go to stderr**.
> Any writes to stdout will corrupt the MCP protocol and break client communication.
>
> Nothing checks this at compile time: a `println!` in a STDIO server compiles
> and then breaks the connection at runtime. Send logs to stderr instead.
>
> **Correct Pattern:**
> ```rust
> // All output goes to stderr via tracing_subscriber
> tracing_subscriber::fmt().with_writer(std::io::stderr).init();
> tracing::info!("message");  // ✅ Goes to stderr
> eprintln!("error");         // ✅ Explicit stderr
> ```
>
> **Wrong Pattern:**
> ```rust
> println!("debug");          // ❌ Written into the protocol stream
> ```

### 🌟 **MCP Enhanced Features**
- **🎵 AudioContent Support** - Multimedia content handling for audio data
- **📝 Enhanced Annotations** - Rich metadata with ISO 8601 timestamp support
- **🏷️ BaseMetadata Pattern** - Proper name/title separation for MCP compliance
- **📋 Advanced Elicitation** - Interactive forms with validation support

### ⚡ **Circuit Breaker & Reliability**
- **Circuit breaker pattern** - Prevents cascade failures
- **Exponential backoff retry** - Intelligent error recovery
- **Connection health monitoring** - Automatic failure detection
- **Graceful degradation** - Fallback mechanisms

### 🔄 **Sharing Patterns for Async Concurrency**
- **Client Clone Pattern** - Directly cloneable (Arc-wrapped internally, no wrapper needed)
- **SharedTransport** - Concurrent transport sharing across async tasks
- **Handler Clone Pattern** - `McpHandler: Clone`, Axum/Tower standard (cheap Arc increments, no wrappers)
- **Generic Shareable Pattern** - Shared<T> and ConsumableShared<T> abstractions
- **Arc/Mutex Encapsulation** - Hide synchronization complexity from public APIs

## Architecture

TurboMCP is built as a layered architecture with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────┐
│                      TurboMCP Framework                     │
│              Ergonomic APIs & Developer Experience         │
├─────────────────────────────────────────────────────────────┤
│                   Infrastructure Layer                     │
│          Server • Client • Transport • Protocol            │
├─────────────────────────────────────────────────────────────┤
│                     Foundation Layer                       │
│             Core Types • Messages • State                  │
└─────────────────────────────────────────────────────────────┘
```

**Components:**
- **[turbomcp-protocol](../turbomcp-protocol/)** - MCP specification implementation, core utilities, and SIMD acceleration
- **[turbomcp-transport](../turbomcp-transport/)** - Multi-protocol transport with circuit breakers
- **[turbomcp-server](../turbomcp-server/)** - Server framework with OAuth 2.0
- **[turbomcp-client](../turbomcp-client/)** - Client implementation with error recovery
- **[turbomcp-macros](../turbomcp-macros/)** - Procedural macros for ergonomic APIs
- **[turbomcp-cli](../turbomcp-cli/)** - Command-line tools for development and testing

## Quick Start

### Installation

Add TurboMCP to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "3.5.0"
tokio = { version = "1.0", features = ["full"] }
```

### Basic Server

Create a simple calculator server:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
        Ok(a + b)
    }

    #[tool("Get server status")]
    async fn status(&self) -> McpResult<String> {
        Ok("Server running".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

### Run the Server

```bash
# Build and run
cargo run

# Test with TurboMCP CLI
cargo install turbomcp-cli

# For HTTP server
turbomcp-cli tools list --url http://localhost:8080/mcp

# For STDIO server
turbomcp-cli tools list --command "./target/debug/my-server"
```

## Type-State Capability Builders

`#[server]` takes no capabilities argument: it advertises exactly what its handlers serve (`tools`, `resources`, `prompts`, `completions` for a `#[completion]` handler, `resources.subscribe` for `#[subscribe]`) plus `logging`, which every server supports. For protocol-level code, or a hand-written `McpHandler::server_capabilities`, `turbomcp-protocol` provides compile-time validated capability builders:

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
- **Compile-time validation** - Invalid configurations caught at build time
- **Zero-cost abstractions** - No runtime overhead for validation
- **Method availability** - Sub-capabilities only available when parent capability is enabled
- **Fluent API** - Readable and maintainable capability configuration
- **Backwards compatibility** - Existing code continues to work unchanged

## Core Concepts

### Server Definition

Use the `#[server]` macro to implement `McpHandler` for your type. The type must be `Clone`, so keep shared state behind an `Arc`:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
struct NotesServer {
    notes: Arc<RwLock<Vec<String>>>,
}

#[server(name = "notes", version = "1.0.0")]
impl NotesServer {
    /// Add a note and return how many there are.
    #[tool]
    async fn add_note(&self, text: String) -> usize {
        let mut notes = self.notes.write().await;
        notes.push(text);
        notes.len()
    }
}
```

### Tool Handlers

Parameters become the tool's input schema, and `#[description]` documents one. Unknown arguments are rejected, as the generated schema declares `additionalProperties: false`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MathServer;

#[server]
impl MathServer {
    /// Round a number.
    #[tool(read_only = true, idempotent = true)]
    async fn round(
        &self,
        #[description("The number to round")]
        value: f64,
        #[description("Decimal places to keep (default 2)")]
        precision: Option<u32>,
    ) -> McpResult<f64> {
        let factor = 10f64.powi(precision.unwrap_or(2) as i32);
        Ok((value * factor).round() / factor)
    }
}
```

Besides the annotation hints (`read_only`, `destructive`, `idempotent`, `open_world`), `#[tool]` accepts `output_schema = Type` and `task_support = "forbidden" | "optional" | "required"`.

### Resource Handlers

A resource handler takes the requested URI and the request context and returns `McpResult<T>`. The URI is the attribute's first argument. A URI template (RFC 6570) matches many URIs; its variables are not bound to parameters, so the handler parses what it needs out of `uri`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Blog;

#[server]
impl Blog {
    /// Application configuration.
    #[resource("config://app", mime_type = "application/json", priority = 0.8)]
    async fn config(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"debug": false}"#.to_string())
    }

    /// One post by one user.
    #[resource("users://{user_id}/posts/{post_id}", mime_type = "text/markdown")]
    async fn post(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        let (user_id, post_id) = uri
            .trim_start_matches("users://")
            .split_once("/posts/")
            .ok_or_else(|| McpError::resource_not_found(&uri))?;
        Ok(format!("post {post_id} for user {user_id}"))
    }
}
```

`#[resource]` also accepts the `ResourceAnnotations` keys `audience = ["user", "assistant"]`, `priority = 0.0..=1.0`, and `last_modified = "2025-01-12T15:00:58Z"`, plus `size = N` (bytes) on a concrete URI.

### Prompt Templates

Prompt arguments are `String` (required) or `Option<String>` (optional), and the handler takes the request context last. The prompt's name is the method name; the `#[prompt("...")]` shorthand sets its description:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Reviewer;

#[server]
impl Reviewer {
    /// Ask for a code review.
    #[prompt]
    async fn code_review(
        &self,
        #[title("Language")]
        #[description("Programming language")]
        language: String,
        #[description("Code to review")]
        code: String,
        ctx: &RequestContext,
    ) -> McpResult<String> {
        Ok(format!("Please review the following {language} code:\n\n{code}"))
    }
}
```

`#[title]` gives an argument a display label for clients that render a form.

### MCP 2025-11-25 Enhanced Features

TurboMCP serves MCP `2025-11-25` and `2025-06-18`, answering each client in
the version it negotiated. Protocol-level features such as resource URI
templates (RFC 6570), elicitation, sampling, tasks, and draft extensions are
implemented in `turbomcp-protocol`; see the crate-level docs for current
surface area.

Beyond `#[tool]`, `#[resource]`, and `#[prompt]`, `#[server]` recognizes markers for the optional MCP methods. Each may appear once, and each advertises the capability it serves:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Docs;

// `page_size` paginates tools/list, resources/list, and prompts/list.
#[server(name = "docs", version = "1.0.0", page_size = 50)]
impl Docs {
    /// A documentation page.
    #[resource("docs://{page}")]
    async fn page(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(format!("contents of {uri}"))
    }

    /// Answers `completion/complete` and advertises `completions`.
    #[completion]
    async fn complete(&self, params: serde_json::Value) -> McpResult<serde_json::Value> {
        let prefix = params["argument"]["value"].as_str().unwrap_or("");
        let values: Vec<&str> = ["intro", "install", "api"]
            .into_iter()
            .filter(|page| page.starts_with(prefix))
            .collect();
        Ok(serde_json::json!({ "completion": { "values": values } }))
    }

    /// Advertises `resources.subscribe`. Must be paired with `#[unsubscribe]`.
    /// Send updates with `ctx.notify_resource_updated(uri)`.
    #[subscribe]
    async fn watch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        Ok(())
    }

    #[unsubscribe]
    async fn unwatch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        Ok(())
    }

    /// Observes `logging/setLevel`. Every server advertises `logging` and
    /// records the level without this; declare it only to react to the change.
    #[set_level]
    async fn level_changed(&self, level: String) -> McpResult<()> {
        eprintln!("client log level is now {level}");
        Ok(())
    }

    /// Runs on `notifications/roots/list_changed`.
    #[roots_changed]
    async fn roots_changed(&self, ctx: &RequestContext) -> McpResult<()> {
        let roots = ctx.list_roots().await?;
        eprintln!("client now exposes {} roots", roots.len());
        Ok(())
    }
}
```

### Context Injection

Add `ctx: &RequestContext` anywhere in a tool's parameter list; resource and prompt handlers always take it. It carries per-request metadata (request ID, transport, session, the authenticated principal, HTTP headers) and the server-to-client operations: `report_progress`, `sample`, `elicit_form` / `elicit_url`, `list_roots`, and `notify_resource_updated` and the other list-changed notifications.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Inspector;

#[server]
impl Inspector {
    /// Describe the current request.
    #[tool]
    async fn inspect(&self, ctx: &RequestContext) -> McpResult<String> {
        if ctx.is_cancelled() {
            return Err(McpError::cancelled("cancelled by client"));
        }
        ctx.report_progress(1.0, Some(1.0), Some("done")).await?;
        Ok(format!(
            "request_id={} transport={:?} subject={:?}",
            ctx.request_id(),
            ctx.transport(),
            ctx.subject(),
        ))
    }
}
```

Log messages to the client (`notifications/message`) go through `turbomcp_protocol::RichContextExt` (`ctx.info(...)`, `ctx.warning(...)`, `ctx.log(level, ...)`), which needs `turbomcp-protocol` as a direct dependency. They are filtered by the level the client set with `logging/setLevel`.

## Authentication & Security

### OAuth 2.1 Setup

TurboMCP ships an OAuth 2.1 + PKCE implementation in the `turbomcp-auth`
crate, re-exported from the main crate as `turbomcp::auth` when the `auth`
feature is enabled. DPoP (RFC 9449) proof-of-possession lives in
`turbomcp-dpop` and is enabled via the `dpop` feature (which pulls in
`auth`). Authenticated identity is attached to requests through
`RequestContext::principal`; tools read it from the context
(`ctx.principal()`, `ctx.subject()`, `ctx.has_any_role(...)`). See the
`turbomcp-auth` crate docs for the provider / middleware construction APIs.

### MCP Authorization (Streamable HTTP)

`ServerConfig::builder().authorization(HttpAuthorization::new(resource, authorization_server, validator))`
makes the HTTP transport an OAuth 2.1 protected resource: it serves RFC 9728
metadata, answers requests without a valid bearer token with a `401`
challenge, and sets the validated principal on each request's context. With
the `auth` and `http` features, `turbomcp::auth::server::JwtBearerValidator`
validates JWTs, audience included. `HttpAuthorization` is exported by
`turbomcp-server`, so add that crate as a direct dependency:

```rust
use turbomcp::auth::jwt::JwtValidator;
use turbomcp::auth::server::JwtBearerValidator;
use turbomcp::prelude::*;
use turbomcp_server::HttpAuthorization;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Who is calling?
    #[tool]
    async fn whoami(&self, ctx: &RequestContext) -> String {
        ctx.subject().unwrap_or("anonymous").to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server's canonical URL: tokens must name it as their audience
    let resource = "https://mcp.example.com/mcp";
    let jwt = JwtValidator::with_jwks_uri(
        "https://auth.example.com".to_string(),
        resource.to_string(),
        "https://auth.example.com/.well-known/jwks.json".to_string(),
    );
    let config = ServerConfig::builder()
        .authorization(HttpAuthorization::new(
            resource,
            "https://auth.example.com",
            JwtBearerValidator::new(jwt).with_required_scopes(["mcp:tools"]),
        ))
        .build();

    turbomcp_server::transport::http::run_with_config(&MyServer, "0.0.0.0:8080", &config).await?;
    Ok(())
}
```

On the client side, set `StreamableHttpClientConfig::auth_provider` to an
`AuthProvider` that supplies the token and answers a server's `401`/`403`
challenge; see the `turbomcp-client` README.

### Security Configuration

Configure HTTP origin policy through the server builder:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Say hello.
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

/// MCP routes to merge into an existing Axum app (needs the `http` feature).
fn mcp_routes() -> axum::Router {
    let config = ServerConfig::builder()
        .allow_origin("https://app.example.com")
        .max_message_size(10 * 1024 * 1024)
        .build();

    MyServer.builder().with_config(config).into_axum_router()
}
```

## Transport Configuration

These examples run `MyServer` from above. Each `run_*` method comes from the
`McpHandlerExt` trait in the prelude and needs its transport's feature.

### STDIO Transport (Default)

Perfect for Claude Desktop and local development:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer.run_stdio().await?;
    Ok(())
}
```

### Streamable HTTP Transport

For web applications and browser integration:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer.run_http("0.0.0.0:8080").await?;
    Ok(())
}
```

### WebSocket Transport

For real-time bidirectional communication:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer.run_websocket("0.0.0.0:8080").await?;
    Ok(())
}
```

### Multi-Transport Runtime Selection

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MyServer;
    
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => server.run_http("0.0.0.0:8080").await?,
        Ok("websocket") => server.run_websocket("0.0.0.0:8080").await?,
        Ok("tcp") => server.run_tcp("0.0.0.0:8080").await?,
        Ok("unix") => server.run_unix("/tmp/mcp.sock").await?,
        _ => server.run_stdio().await?, // Default
    }
    Ok(())
}
```

## Cloning & Concurrency Patterns

TurboMCP provides clean concurrency patterns with Arc-wrapped internals:

### Client Clone Pattern - Direct Cloning (No Wrapper Needed)

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Client is directly cloneable (Arc-wrapped internally); needs `full-client`
    let client = Client::connect_http("http://localhost:8080").await?;

    // Clone for concurrent usage (cheap Arc increments)
    let client1 = client.clone();
    let client2 = client.clone();

    // Both tasks can access the client concurrently
    let handle1 = tokio::spawn(async move { client1.list_tools().await });
    let handle2 = tokio::spawn(async move { client2.list_prompts().await });

    let (tools, prompts) = tokio::join!(handle1, handle2);
    Ok(())
}
```

### SharedTransport - Concurrent Transport Access

```rust
use turbomcp::MessageId;
use turbomcp_transport::{SharedTransport, StdioTransport, TransportMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Wrap any transport for sharing across tasks
    let shared = SharedTransport::new(StdioTransport::new());

    // Connect once
    shared.connect().await?;

    let sender = shared.clone();
    let receiver = shared.clone();

    let send = tokio::spawn(async move {
        let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        sender.send(TransportMessage::new(MessageId::from(1), ping.into())).await
    });
    let receive = tokio::spawn(async move { receiver.receive().await });

    let _ = tokio::join!(send, receive);
    Ok(())
}
```

### Generic Shareable Pattern

```rust
use turbomcp_protocol::shared::{ConsumableShared, Shareable, Shared};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Any type can be made shareable
    let shared = Shared::new(Vec::<String>::new());

    // Use with closures for fine-grained control
    shared.with_mut(|items| items.push("first".to_string())).await;
    let count = shared.with(|items| items.len()).await;

    // Consumable variant for one-time use
    let shared = ConsumableShared::new(String::from("config"));
    let value = shared.consume().await?; // Extracts the value
    Ok(())
}
```

### Benefits
- **Clean APIs**: No exposed Arc/Mutex types
- **Easy Sharing**: Clone for concurrent access
- **Thread Safety**: Built-in synchronization
- **Zero Overhead**: Same performance as direct usage
- **MCP Compliant**: Preserves all protocol semantics

## Error Handling

### Error Architecture

TurboMCP exposes a single unified error type — `McpError` — defined in
`turbomcp-core` and re-exported as `turbomcp::McpError` /
`turbomcp::McpResult`. There is **one error type across the whole stack:**
handlers, middleware, transport, and protocol layers all speak the same
`McpError`.

This is a deliberate simplification over the earlier two-tier
(`McpError` → `ProtocolError`) design: `McpError` already carries
JSON-RPC error codes, HTTP status mapping, retryability metadata, and a
fluent `.with_operation(...)` / `.with_component(...)` context chain,
which is everything the old `ProtocolError` provided.

#### Flow

```
Your Tool Handler
    ↓ returns McpResult<T> (i.e. Result<T, McpError>)
Server Layer (turbomcp-server)
    ↓ inspects McpError metadata (jsonrpc_code, retryability, context)
Protocol / JSON-RPC Response
```

Use `McpError` everywhere. For MCP-specification error codes, the
appropriate constructor (`tool_not_found`, `invalid_params`,
`resource_not_found`, `authentication`, `permission_denied`,
`rate_limited`, `timeout`, `transport`, `internal`, …) picks the right
JSON-RPC / MCP code for you — see the "Error Handling" examples below.

### Ergonomic Error Creation

Use `McpError` constructors for error creation. A tool's error reaches the
client as a tool execution error (`isError: true`), so the model can see it
and correct its call; the error kind is kept in `_meta`. A resource or prompt
error is a JSON-RPC error.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server]
impl Files {
    #[tool("Divide numbers")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(McpError::invalid_params(format!("Division by zero: {} / {}", a, b)));
        }
        Ok(a / b)
    }

    #[tool("Read file")]
    async fn read_file(&self, path: String) -> McpResult<String> {
        tokio::fs::read_to_string(&path).await
            .map_err(|e| McpError::internal(format!("Failed to read file {}: {}", path, e)))
    }
}
```

### Application-Level Errors (`McpError`)

Construct errors with fluent constructors:

```rust
use turbomcp::McpError;

// Construct with appropriate constructor
let err = McpError::invalid_params("Name must not be empty");
let err = McpError::authentication("Token expired");
let err = McpError::resource_not_found("file://missing.txt");
let err = McpError::transport("Connection dropped");
let err = McpError::internal("Unexpected state")
    .with_operation("process")
    .with_component("handler");

// Query error metadata
assert!(err.is_retryable() || !err.is_retryable());
let _code = err.jsonrpc_code();
let _status = err.http_status();
```

### Protocol-Level Error Codes

`McpError` exposes its MCP / JSON-RPC semantics directly — no separate
error type is needed:

```rust
use turbomcp::McpError;

let err = McpError::internal("Database connection failed")
    .with_operation("user_lookup")
    .with_component("auth_service");

assert_eq!(err.jsonrpc_code(), -32603);   // Internal error
let _http_status = err.http_status();     // HTTP mapping
let _retryable = err.is_retryable();      // Retry hint for clients
```

Constructors such as `tool_not_found`, `invalid_params`,
`resource_not_found`, `capability_not_supported`, and `rate_limited`
emit the MCP-spec JSON-RPC codes defined in `turbomcp-core::error_codes`.

## Advanced Features

### Custom Types and Schema Generation

TurboMCP generates JSON schemas for custom types that derive
`schemars::JsonSchema` (add `schemars = "1"` to your dependencies). Returning
`Json<T>` sends the value as `structuredContent` and advertises `T`'s schema as
the tool's `outputSchema`:

```rust
use schemars::JsonSchema;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use turbomcp::prelude::*;

#[derive(Deserialize, JsonSchema)]
struct CreateUserRequest {
    name: String,
    email: String,
    age: Option<u32>,
}

#[derive(Serialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[derive(Clone, Default)]
struct Users {
    next_id: Arc<AtomicU64>,
}

#[server]
impl Users {
    #[tool("Create a new user")]
    async fn create_user(&self, request: CreateUserRequest) -> McpResult<Json<User>> {
        // Input and output schemas are generated from both types
        Ok(Json(User {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            name: request.name,
            email: request.email,
        }))
    }
}
```

### Graceful Shutdown

The HTTP transport integrates with Tokio signal handlers for graceful
shutdown. Configure the drain timeout through the server builder
(`ServerBuilder::with_graceful_shutdown`); the HTTP runner awaits SIGINT
(and SIGTERM on Unix) and drains in-flight requests up to the configured
deadline. For STDIO, the process exits cleanly when stdin closes.

### Performance Tuning

SIMD-accelerated JSON parsing is provided by `turbomcp-protocol` (enabled by default via its `simd` feature, which selects `sonic-rs`). No extra flag is required on the `turbomcp` crate.

Configure server behavior via `ServerConfig` and pass it through the
server builder — the convenience methods (`run_stdio`, `run_http`, …)
use defaults and ignore any standalone `ServerConfig`, so reach for
`.builder().with_config(...)` when you need custom settings. With
`Calculator` from the Quick Start:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::builder()
        .max_message_size(10 * 1024 * 1024)   // 10 MB
        .build();

    Calculator
        .builder()
        .with_config(config)
        .transport(Transport::stdio())         // or http/tcp/websocket/unix
        .serve()
        .await?;
    Ok(())
}
```

HTTP authorization is the exception: run a server that needs it with
`turbomcp_server::transport::http::run_with_config`, as shown under
[MCP Authorization](#mcp-authorization-streamable-http).

## Testing

### Unit Testing

Test your tools directly by calling them as normal methods, or through
`McpTestClient`, which dispatches like a real client without a transport.
With `Calculator` from the Quick Start:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp::prelude::*;

    #[tokio::test]
    async fn test_calculator() {
        // Call the tool method directly
        let result = Calculator.add(5, 3).await.unwrap();
        assert_eq!(result, 8);

        // Or go through MCP dispatch, argument validation included
        let client = McpTestClient::new(Calculator);
        let result = client
            .call_tool("add", serde_json::json!({"a": 5, "b": 3}))
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("8"));
    }
}
```

### Integration Testing

Use the TurboMCP CLI for integration testing:

```bash
# Install CLI
cargo install turbomcp-cli

# Test server functionality
turbomcp-cli tools list --url http://localhost:8080/mcp
turbomcp-cli tools call add --arguments '{"a": 5, "b": 3}' --url http://localhost:8080/mcp
turbomcp-cli tools schema --url http://localhost:8080/mcp

# Test STDIO server
turbomcp-cli tools list --command "./target/debug/my-server"
turbomcp-cli resources list --command "./target/debug/my-server"
```

## Client Setup

### Claude Desktop

Add to your Claude Desktop configuration:

```json
{
  "mcpServers": {
    "my-turbomcp-server": {
      "command": "/path/to/your/server/binary",
      "args": []
    }
  }
}
```

### Programmatic Client

Use the TurboMCP client (the `full-client` feature, or `turbomcp-client` directly):

```rust
use std::collections::HashMap;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect over HTTP to <base>/mcp and initialize. Also: Client::connect_tcp,
    // Client::connect_unix, or Client::new(transport) followed by initialize().
    let client = Client::connect_http("http://localhost:8080").await?;

    let tools = client.list_tools().await?;
    println!("Available tools: {:?}", tools);

    let mut args = HashMap::new();
    args.insert("a".into(), serde_json::json!(5));
    args.insert("b".into(), serde_json::json!(3));

    // call_tool(name, arguments, task_metadata)
    let result = client.call_tool("add", Some(args), None).await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

## Examples

Explore examples in the `examples/` directory:

```bash
# Minimal server
cargo run --example hello_world
cargo run --example calculator
cargo run --example macro_server

# Server patterns
cargo run --example stateful
cargo run --example validation
cargo run --example composition
cargo run --example middleware
cargo run --example visibility
cargo run --example tags_versioning

# Transports (require the matching feature flag)
cargo run --example tcp_server  --features tcp
cargo run --example tcp_client  --features tcp,full-client
cargo run --example unix_server --features unix
cargo run --example unix_client --features unix,full-client
cargo run --example transports_demo --features "stdio,http,tcp"

# Capability builders & testing
cargo run --example type_state_builders_demo
cargo run --example test_client
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `stdio` | STDIO transport | ✅ |
| `http` | HTTP / SSE (Streamable HTTP) transport | ❌ |
| `websocket` | WebSocket bidirectional transport | ❌ |
| `tcp` | Raw TCP socket transport | ❌ |
| `unix` | Unix domain socket transport | ❌ |
| `channel` | In-process channel transport (testing/benchmarks) | ❌ |
| `minimal` | Bundle: STDIO only (= `stdio`) | ❌ |
| `full` | Bundle: all transports + telemetry | ❌ |
| `full-stack` | Bundle: `full` + `full-client` | ❌ |
| `all-transports` | Bundle: all transports incl. `channel` (no telemetry) | ❌ |
| `telemetry` | OpenTelemetry, metrics, structured logging | ❌ |
| `auth` | OAuth 2.1, JWT, API key auth (turbomcp-auth) | ❌ |
| `dpop` | RFC 9449 DPoP (requires `auth`) | ❌ |
| `client-integration` | Re-export minimal `turbomcp-client` (STDIO) | ❌ |
| `full-client` | `turbomcp-client` with all transports | ❌ |
| `experimental-tasks` | Tasks API (SEP-1686) | ❌ |

### Important: Minimum Feature Requirements

When using `default-features = false`, you must explicitly enable at least one transport feature to have a functional MCP server. The available transport features are:

- `stdio` - STDIO transport (included in default features)
- `http` - Streamable HTTP transport
- `websocket` - WebSocket transport
- `tcp` - TCP transport
- `unix` - Unix socket transport

**Example configurations:**

```toml
# Minimal STDIO-only server
[dependencies]
turbomcp = { version = "3.5.0", default-features = false, features = ["stdio"] }

# HTTP-only server
[dependencies]
turbomcp = { version = "3.5.0", default-features = false, features = ["http"] }

# Multiple transports without default features
[dependencies]
turbomcp = { version = "3.5.0", default-features = false, features = ["stdio", "http", "websocket"] }
```

Without at least one transport feature enabled, the server will not be able to communicate using the MCP protocol.

## Development

### Building

```bash
# Build with all features
cargo build --all-features

# Build optimized for production (SIMD JSON is enabled by default via turbomcp-protocol)
cargo build --release --features full

# Run tests
cargo test --workspace
```

### Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes and add tests
4. Run the full test suite: `just test`
5. Submit a pull request

## Performance Architecture

### Compile-Time Optimization

TurboMCP uses a compile-time first approach with these characteristics:

**Build-Time Features:**
- Macro-driven code generation pre-computes metadata at build time
- Tool schemas, parameter validation, and handler dispatch tables generated statically
- Rust's type system provides compile-time safety and optimization opportunities
- Feature flags allow selective compilation for lean binaries

**Runtime Characteristics:**
- Static schema generation eliminates per-request computation
- Direct function dispatch without hash table lookups
- Zero-copy message handling where possible
- Async runtime scaling with Tokio

**Implementation Approach:**
```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Adder;

// Compile-time schema generation
#[server]
impl Adder {
    #[tool("Add numbers")]
    async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
        Ok(a + b)  // Schema and dispatch code generated at build time
    }
}
```

### Benchmarks

```bash
# Run performance benchmarks
cargo bench
```

## Documentation

- **[Architecture Guide](../../ARCHITECTURE.md)** - System design and components
- **[Security Features](../turbomcp-transport/SECURITY_FEATURES.md)** - Comprehensive security documentation
- **[API Documentation](https://docs.rs/turbomcp)** - Complete API reference
- **[Examples](./examples/)** - Ready-to-use code examples

## Related Projects

- **[Model Context Protocol](https://modelcontextprotocol.io/)** - Official protocol specification
- **[Claude Desktop](https://claude.ai)** - AI assistant with MCP support
- **[MCP Servers](https://github.com/modelcontextprotocol/servers)** - Official server implementations

## License

Licensed under the [MIT License](../../LICENSE).

---

*Built with ❤️ by the TurboMCP team*
