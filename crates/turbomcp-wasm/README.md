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
    name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs {
    a: i64,
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
