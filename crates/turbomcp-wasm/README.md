# turbomcp-wasm

WebAssembly bindings for TurboMCP - MCP client and server for browsers, edge environments, and WASI runtimes.

## Write Once, Run Everywhere

TurboMCP v3 supports **portable MCP handlers** that can run on both native platforms (Linux, macOS, Windows) and WASM/edge environments with zero code changes. Write your `McpHandler` implementation once, then deploy to any target:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyHandler;

#[turbomcp::server(name = "my-server", version = "1.0.0")]
impl MyHandler {
    #[tool(description = "Say hello")]
    async fn hello(&self, name: String) -> String {
        format!("Hello, {}!", name)
    }
}

// Native: Use run_tcp(), run_http(), run_websocket(), or serve() (stdio)
// WASM: Use WasmHandlerExt trait for Cloudflare Workers
```

### WASM Deployment with WasmHandlerExt

Any `McpHandler` can be used directly in Cloudflare Workers via the `WasmHandlerExt` extension trait:

```rust
use turbomcp_wasm::wasm_server::WasmHandlerExt;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let handler = MyHandler;
    handler.handle_worker_request(req).await
}
```

This enables sharing business logic between native servers and edge deployments without code duplication.

## Features

### Client (Browser & WASI)

- **Browser Support**: Full MCP client using Fetch API and WebSocket
- **Type-Safe**: All MCP types available in JavaScript/TypeScript
- **Async/Await**: Modern Promise-based API
- **Small Binary**: Optimized for minimal bundle size (~50-200KB)

### Server (wasm-server feature)

- **Edge MCP Servers**: Build MCP servers running on Cloudflare Workers and other edge platforms
- **Ergonomic API**: Just write async functions - inspired by axum's IntoResponse pattern
- **Type-Safe Handlers**: Automatic JSON schema generation from Rust types
- **Idiomatic Error Handling**: Full `?` operator support
- **Zero Tokio**: Uses wasm-bindgen-futures for async, no tokio runtime needed
- **Full MCP Protocol**: Tools, resources, prompts, and all standard MCP methods

## Installation

### Client (NPM)

```bash
npm install turbomcp-wasm
```

### Server (Rust)

```toml
[dependencies]
turbomcp-wasm = { version = "3.0", default-features = false, features = ["wasm-server"] }
worker = "0.7"
serde = { version = "1.0", features = ["derive"] }
schemars = "1.0"
getrandom = { version = "0.3", features = ["wasm_js"] }
```

## Client Usage

### Browser (ES Modules)

```javascript
import init, { McpClient } from 'turbomcp-wasm';

async function main() {
  // Initialize WASM module
  await init();

  // Create client
  const client = new McpClient("https://api.example.com/mcp")
    .withAuth("your-api-token")
    .withTimeout(30000);

  // Initialize session
  await client.initialize();

  // List available tools
  const tools = await client.listTools();
  console.log("Tools:", tools);

  // Call a tool
  const result = await client.callTool("my_tool", {
    param1: "value1",
    param2: 42
  });
  console.log("Result:", result);
}

main().catch(console.error);
```

### TypeScript

```typescript
import init, { McpClient, Tool, Resource, Content } from 'turbomcp-wasm';

async function main(): Promise<void> {
  await init();

  const client = new McpClient("https://api.example.com/mcp");
  await client.initialize();

  const tools: Tool[] = await client.listTools();
  const resources: Resource[] = await client.listResources();
}
```

## Server Usage (Cloudflare Workers)

### Basic Server - Ergonomic API

```rust
use turbomcp_wasm::wasm_server::*;
use worker::*;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {
    /// The name of the person to greet
    name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs {
    /// The first number
    a: i64,
    /// The second number
    b: i64,
}

// Just write async functions - return any type implementing IntoToolResponse!
async fn hello(args: HelloArgs) -> String {
    format!("Hello, {}!", args.name)
}

async fn add(args: AddArgs) -> String {
    format!("{}", args.a + args.b)
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-mcp-server", "1.0.0")
        .description("My MCP server running on Cloudflare Workers")
        .tool("hello", "Say hello to someone", hello)
        .tool("add", "Add two numbers", add)
        .build();

    server.handle(req).await
}
```

### With Error Handling

```rust
use turbomcp_wasm::wasm_server::*;

#[derive(Deserialize, schemars::JsonSchema)]
struct DivideArgs {
    a: f64,
    b: f64,
}

// Use Result for error handling - errors automatically become tool errors
async fn divide(args: DivideArgs) -> Result<String, ToolError> {
    if args.b == 0.0 {
        return Err(ToolError::new("Cannot divide by zero"));
    }
    Ok(format!("{}", args.a / args.b))
}

// Use the ? operator for automatic error propagation
async fn fetch_data(args: FetchArgs) -> Result<Json<Data>, ToolError> {
    let response = fetch(&args.url).await?;  // ? just works!
    let data: Data = response.json().await?;
    Ok(Json(data))
}
```

### Parameter Descriptions

Add descriptions to tool parameters using doc comments or schemars attributes:

```rust
use serde::Deserialize;
use schemars::JsonSchema;

// Option 1: Doc comments (preferred)
#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    /// The search query to execute
    query: String,
    /// Maximum number of results to return (default: 10)
    limit: Option<u32>,
}

// Option 2: schemars attribute
#[derive(Deserialize, JsonSchema)]
struct FilterArgs {
    #[schemars(description = "Field to filter on")]
    field: String,
    #[schemars(description = "Filter operator (eq, gt, lt, contains)")]
    operator: String,
}
```

These descriptions appear in the JSON schema and help LLMs understand how to use your tools correctly.

### Return Type Flexibility

```rust
use turbomcp_wasm::wasm_server::*;

// Return String
async fn simple(args: Args) -> String {
    "Hello!".into()
}

// Return JSON
async fn json_response(args: Args) -> Json<MyData> {
    Json(MyData { value: 42 })
}

// Return Result with automatic error handling
async fn fallible(args: Args) -> Result<String, ToolError> {
    let data = some_operation()?;
    Ok(format!("Got: {}", data))
}

// Return ToolResult for full control
async fn full_control(args: Args) -> ToolResult {
    ToolResult::text("Direct response")
}

// Return () for empty success
async fn void_response(_: Args) -> () {
    // Do something with side effects
}

// Return Option
async fn optional(args: Args) -> Option<String> {
    if args.enabled {
        Some("Enabled".into())
    } else {
        None  // Returns "No result"
    }
}
```

### With Authentication (Cloudflare Access)

```rust
use turbomcp_wasm::wasm_server::*;
use turbomcp_wasm::auth::CloudflareAccessAuthenticator;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Get secrets from Cloudflare environment (never hardcode!)
    let team_name = env.var("CLOUDFLARE_ACCESS_TEAM")?.to_string();
    let audience = env.var("CLOUDFLARE_ACCESS_AUDIENCE")?.to_string();

    let server = McpServer::builder("protected-server", "1.0.0")
        .tool("hello", "Say hello", hello_handler)
        .build();

    // Wrap with Cloudflare Access authentication
    let auth = CloudflareAccessAuthenticator::new(&team_name, &audience);
    let protected = server.with_auth(auth);

    protected.handle(req).await
}
```

**Important**: Always use `worker::Env` to retrieve secrets at runtime. Never hardcode credentials in your code.

### Secret Management Best Practices

```rust
use turbomcp_wasm::wasm_server::*;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // ✅ GOOD: Retrieve secrets from environment
    let api_key = env.secret("API_KEY")?.to_string();
    let db_url = env.var("DATABASE_URL")?.to_string();

    // ❌ BAD: Never do this
    // let api_key = "sk-secret-key-123";  // NEVER hardcode secrets!

    // Capture secrets in closure for use in handlers
    let server = McpServer::builder("my-server", "1.0.0")
        .tool("query", "Query data", move |args: QueryArgs| {
            let key = api_key.clone();
            async move {
                // Use the captured secret
                fetch_with_auth(&key, &args.query).await
            }
        })
        .build();

    server.handle(req).await
}
```

Configure secrets in `wrangler.toml`:

```toml
[vars]
DATABASE_URL = "https://db.example.com"
CLOUDFLARE_ACCESS_TEAM = "your-team"
CLOUDFLARE_ACCESS_AUDIENCE = "your-aud-tag"

# Secrets should be set via wrangler CLI, not in config:
# wrangler secret put API_KEY
```

## Security

TurboMCP WASM implements defense-in-depth security measures to protect against common JWT and authentication vulnerabilities.

### JWT Security Features

**Algorithm Confusion Attack Prevention:**
- Algorithm whitelist is mandatory - tokens are rejected if no algorithms are configured
- Key-type validation ensures RSA keys can only be used with RS* algorithms, EC keys with ES* algorithms
- The `"none"` algorithm is always rejected

```rust
use turbomcp_wasm::auth::{JwtConfig, JwtAlgorithm};

// ✅ CORRECT: Always use JwtConfig::new() for secure defaults
let config = JwtConfig::new()  // Defaults to [RS256, ES256]
    .issuer("https://auth.example.com")
    .audience("my-api");

// ✅ CORRECT: Or explicitly specify algorithms
let config = JwtConfig::new()
    .algorithms(vec![JwtAlgorithm::RS256]);

// ❌ WRONG: Never create config with empty algorithms
// The Default trait is NOT implemented to prevent this mistake
```

**JWKS Security:**
- JWKS URLs must use HTTPS (HTTP is rejected to prevent MITM attacks)
- Localhost URLs are allowed for development
- Use `allow_insecure_http()` only for testing (never in production)

```rust
use turbomcp_wasm::auth::JwksCache;

// ✅ CORRECT: HTTPS URL
let cache = JwksCache::new("https://auth.example.com/.well-known/jwks.json");

// ✅ OK: Localhost for development
let cache = JwksCache::new("http://localhost:8080/.well-known/jwks.json");

// ⚠️ DANGER: Only for testing!
let cache = JwksCache::new("http://test-server/.well-known/jwks.json")
    .allow_insecure_http();
```

**Claim Validation:**
- Expiration (`exp`) validation is enabled by default
- Not-before (`nbf`) validation is enabled by default
- Issuer (`iss`) and audience (`aud`) validation when configured
- 60-second clock skew leeway (configurable)

### Request Security

- Maximum request body size: 1MB (DoS protection)
- POST-only enforcement for JSON-RPC
- Content-Type validation
- Strict JSON-RPC 2.0 compliance

### CORS Security

TurboMCP implements secure CORS handling to prevent credentials from being exposed to arbitrary origins:

- **Origin Echo**: The request `Origin` header is echoed back in `Access-Control-Allow-Origin` instead of using `*`
- **Vary Header**: `Vary: Origin` is automatically added when origin-specific responses are returned (required for proper caching)
- **Non-Browser Fallback**: Only falls back to `*` when no Origin header is present (e.g., curl, Postman)

This approach:
- Prevents credentials leakage to malicious sites
- Enables proper CORS preflight validation
- Works correctly with CDN caching

```http
# Browser request with Origin header
Request:
Origin: https://app.example.com

Response:
Access-Control-Allow-Origin: https://app.example.com
Vary: Origin
Access-Control-Allow-Methods: POST, OPTIONS
Access-Control-Allow-Headers: Content-Type, Authorization, X-Request-ID
Access-Control-Max-Age: 86400

# Non-browser request (no Origin header)
Response:
Access-Control-Allow-Origin: *
# (No Vary header when using wildcard)
```

### OAuth and Token Protection

TurboMCP supports multiple authentication patterns for protecting MCP servers:

**1. Cloudflare Access (Recommended for Production)**

Cloudflare Access provides enterprise-grade zero-trust authentication with automatic key rotation:

```rust
use turbomcp_wasm::auth::CloudflareAccessAuthenticator;

let auth = CloudflareAccessAuthenticator::new("your-team", "your-audience-tag");
let protected = server.with_auth(auth);
```

**2. Custom JWT Validation**

For self-hosted OAuth/OIDC providers:

```rust
use turbomcp_wasm::auth::{JwtValidator, JwksCache, JwtConfig, JwtAlgorithm};

// Configure JWT validation
let config = JwtConfig::new()
    .algorithms(vec![JwtAlgorithm::RS256, JwtAlgorithm::ES256])
    .issuer("https://auth.example.com")
    .audience("your-api");

// Set up JWKS caching for signature verification
let jwks = JwksCache::new("https://auth.example.com/.well-known/jwks.json");

// Create validator
let validator = JwtValidator::new(config, jwks);
```

**3. Bearer Token (Development Only)**

For simple API key authentication during development:

```rust
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Extract Bearer token
    let auth_header = req.headers().get("Authorization")?;
    let expected_key = env.secret("API_KEY")?.to_string();

    if auth_header != Some(format!("Bearer {}", expected_key)) {
        return Response::error("Unauthorized", 401);
    }

    // Process authenticated request
    server.handle(req).await
}
```

**⚠️ Warning**: Simple Bearer tokens lack rotation and are vulnerable to theft. Use OAuth/OIDC for production.

### Cloudflare Access Integration

When using Cloudflare Access, the `CloudflareAccessAuthenticator` enforces additional security:

- Only RS256 algorithm is allowed
- JWKS fetched from Cloudflare's official endpoint
- Validates `Cf-Access-Jwt-Assertion` header (or falls back to `Authorization: Bearer`)

```rust
use turbomcp_wasm::auth::CloudflareAccessAuthenticator;

// CloudflareAccessAuthenticator automatically:
// - Uses HTTPS JWKS endpoint
// - Restricts to RS256 only
// - Validates issuer and audience
let auth = CloudflareAccessAuthenticator::new("your-team", "your-audience-tag");
```

### Path and URI Security

TurboMCP WASM runs in edge environments (Cloudflare Workers, Deno Deploy) where there is typically no filesystem access. Resources are URIs pointing to:
- KV stores (`kv://namespace/key`)
- Durable Objects (`do://object/path`)
- External APIs (`https://api.example.com/resource`)
- D1 databases (`d1://database/table`)

**Path Traversal Protection**: When implementing resource handlers that map URIs to storage backends, you are responsible for validating and sanitizing URIs to prevent unauthorized access:

```rust
use turbomcp_wasm::wasm_server::*;

async fn read_user_data(uri: String) -> Result<ResourceResult, ToolError> {
    // Extract the user ID from the URI
    let id = uri
        .strip_prefix("user://")
        .ok_or_else(|| ToolError::new("Invalid URI scheme"))?;

    // SECURITY: Validate the ID to prevent directory traversal
    // Only allow alphanumeric characters and hyphens
    if !id.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return Err(ToolError::new("Invalid user ID format"));
    }

    // Now safe to use in KV key
    let kv_key = format!("users/{}", id);
    // ... fetch from KV
    Ok(ResourceResult::text(&uri, "user data"))
}
```

**Best Practices:**
- Validate URI schemes (reject unexpected prefixes)
- Sanitize path segments (reject `..`, absolute paths, special characters)
- Use allowlists for valid characters when possible
- Log suspicious URI patterns for security monitoring

### WASM Architecture Considerations

TurboMCP uses `MaybeSend`/`MaybeSync` marker traits to support both native and WASM environments:

- **Native**: Requires `Send + Sync` for multi-threaded executors (tokio, etc.)
- **WASM**: No thread-safety requirements (single-threaded execution model)

This architecture allows you to use `Rc<RefCell<T>>` for shared state in WASM handlers:

```rust
use std::rc::Rc;
use std::cell::RefCell;

// This works in WASM but would NOT compile for native targets
struct WasmOnlyHandler {
    state: Rc<RefCell<Vec<String>>>,
}
```

For portable code that works on both native and WASM, use `Arc<RwLock<T>>` instead.

### Security Checklist

- [ ] Use `JwtConfig::new()` instead of manual construction
- [ ] Always configure `issuer` and `audience` in JwtConfig
- [ ] Use HTTPS for all JWKS endpoints
- [ ] Store secrets using `env.secret()`, never hardcode
- [ ] Use Cloudflare Access for production deployments
- [ ] Configure rate limiting at the Cloudflare level
- [ ] CORS: Origin is echoed by default for security; `*` is only used for non-browser clients
- [ ] Validate and sanitize URIs in resource handlers
- [ ] Use allowlists for URI path components where possible

### With Resources and Prompts

```rust
use turbomcp_wasm::wasm_server::*;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("full-server", "1.0.0")
        // Tools - ergonomic API
        .tool("search", "Search the database", search_handler)

        // Static resource
        .resource(
            "config://settings",
            "Application Settings",
            "Current application configuration",
            |_uri| async move {
                ResourceResult::json("config://settings", &serde_json::json!({
                    "theme": "dark",
                    "language": "en"
                }))
            },
        )

        // Dynamic resource template
        .resource_template(
            "user://{id}",
            "User Profile",
            "Get user profile by ID",
            |uri| async move {
                let id = uri.split('/').last().unwrap_or("unknown");
                Ok(ResourceResult::text(&uri, format!("User {}", id)))
            },
        )

        // Prompt with no arguments
        .prompt_no_args(
            "greeting",
            "Generate a greeting",
            || async move {
                PromptResult::user("Hello! How can I help?")
            },
        )

        .build();

    server.handle(req).await
}
```

## API Reference

### Client Methods

| Method | Description |
|--------|-------------|
| `withAuth(token: string)` | Add Bearer token authentication |
| `withHeader(key: string, value: string)` | Add custom header |
| `withTimeout(ms: number)` | Set request timeout |
| `initialize()` | Initialize MCP session |
| `isInitialized()` | Check if session is initialized |
| `getServerInfo()` | Get server implementation info |
| `getServerCapabilities()` | Get server capabilities |
| `listTools()` | List available tools |
| `callTool(name: string, args?: object)` | Call a tool |
| `listResources()` | List available resources |
| `readResource(uri: string)` | Read a resource |
| `listResourceTemplates()` | List resource templates |
| `listPrompts()` | List available prompts |
| `getPrompt(name: string, args?: object)` | Get a prompt |
| `ping()` | Ping the server |

### Server Builder Methods

| Method | Description |
|--------|-------------|
| `builder(name, version)` | Create new server builder |
| `description(text)` | Set server description |
| `instructions(text)` | Set server instructions |
| `tool(name, desc, handler)` | Register tool (ergonomic API) |
| `tool_no_args(name, desc, handler)` | Register tool without arguments |
| `tool_raw(name, desc, handler)` | Register tool with raw JSON args |
| `resource(uri, name, desc, handler)` | Register static resource |
| `resource_template(uri, name, desc, handler)` | Register resource template |
| `prompt(name, desc, handler)` | Register prompt with typed args |
| `prompt_no_args(name, desc, handler)` | Register prompt without args |
| `build()` | Build the server |

### Return Types (IntoToolResponse)

| Type | Behavior |
|------|----------|
| `String`, `&str` | Returns as text content |
| `Json<T>` | Serializes to JSON text |
| `ToolResult` | Full control over response |
| `Result<T, E>` | `Ok` becomes success, `Err` becomes error |
| `()` | Empty success response |
| `Option<T>` | `None` returns "No result" |
| `(A, B)` | Combines multiple contents |

### Error Types

| From Type | Conversion |
|-----------|------------|
| `std::io::Error` | Auto-converts to ToolError |
| `serde_json::Error` | Auto-converts to ToolError |
| `String`, `&str` | Direct message |
| `Box<dyn Error>` | Auto-converts to ToolError |
| `worker::Error` | Via `WorkerError` wrapper or `WorkerResultExt` trait |

### Worker Error Integration

Due to Rust's orphan rules, `worker::Error` cannot directly convert to `ToolError`. TurboMCP provides two ergonomic solutions:

**Option 1: WorkerError wrapper**

```rust
use turbomcp_wasm::wasm_server::{ToolError, WorkerError};

async fn kv_handler(args: Args, env: &Env) -> Result<String, ToolError> {
    let kv = env.kv("MY_KV").map_err(WorkerError)?;
    let value = kv.get(&args.key).text().await.map_err(WorkerError)?;
    Ok(value.unwrap_or_default())
}
```

**Option 2: WorkerResultExt trait (more ergonomic)**

```rust
use turbomcp_wasm::wasm_server::{ToolError, WorkerResultExt};

async fn kv_handler(args: Args, env: &Env) -> Result<String, ToolError> {
    let kv = env.kv("MY_KV").into_tool_result()?;
    let value = kv.get(&args.key).text().await.into_tool_result()?;
    Ok(value.unwrap_or_default())
}
```

Both approaches enable full `?` operator support when working with Cloudflare Workers APIs (KV, Durable Objects, R2, D1, etc.).

## Binary Size

| Configuration | Size |
|--------------|------|
| Core types only | ~50KB |
| + JSON serialization | ~100KB |
| + HTTP client | ~200KB |
| wasm-server feature | ~536KB |

## Browser Compatibility

- Chrome 89+
- Firefox 89+
- Safari 15+
- Edge 89+

Requires support for:
- WebAssembly
- Fetch API
- ES2020 (async/await)

## WASI Support

WASI Preview 2 support for running in server-side WASM runtimes:
- Wasmtime 29+
- WasmEdge
- Wasmer

## License

MIT
