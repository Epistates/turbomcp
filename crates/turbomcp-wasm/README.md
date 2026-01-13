# turbomcp-wasm

WebAssembly bindings for TurboMCP - MCP client and server for browsers, edge environments, and WASI runtimes.

## Features

### Client (Browser & WASI)

- **Browser Support**: Full MCP client using Fetch API and WebSocket
- **Type-Safe**: All MCP types available in JavaScript/TypeScript
- **Async/Await**: Modern Promise-based API
- **Small Binary**: Optimized for minimal bundle size (~50-200KB)

### Server (wasm-server feature)

- **Edge MCP Servers**: Build MCP servers running on Cloudflare Workers and other edge platforms
- **Type-Safe Handlers**: Automatic JSON schema generation from Rust types
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

### Basic Server

```rust
use turbomcp_wasm::wasm_server::{McpServer, ToolResult};
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

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-mcp-server", "1.0.0")
        .description("My MCP server running on Cloudflare Workers")
        .with_tool("hello", "Say hello to someone", |args: HelloArgs| async move {
            Ok(ToolResult::text(format!("Hello, {}!", args.name)))
        })
        .with_tool("add", "Add two numbers", |args: AddArgs| async move {
            Ok(ToolResult::text(format!("{}", args.a + args.b)))
        })
        .build();

    server.handle(req).await
}
```

### With Resources and Prompts

```rust
use turbomcp_wasm::wasm_server::{McpServer, ToolResult, ResourceResult, PromptResult};
use turbomcp_core::types::prompts::PromptArgument;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("full-server", "1.0.0")
        // Tools
        .with_tool("search", "Search the database", |args: SearchArgs| async move {
            Ok(ToolResult::text("Search results..."))
        })

        // Static resource
        .with_resource(
            "config://settings",
            "Application Settings",
            "Current application configuration",
            |_uri| async move {
                Ok(ResourceResult::json("config://settings", &serde_json::json!({
                    "theme": "dark",
                    "language": "en"
                }))?)
            },
        )

        // Dynamic resource template
        .with_resource_template(
            "user://{id}",
            "User Profile",
            "Get user profile by ID",
            |uri| async move {
                let id = uri.split('/').last().unwrap_or("unknown");
                Ok(ResourceResult::text(&uri, format!("User {}", id)))
            },
        )

        // Prompt
        .with_prompt(
            "greeting",
            "Generate a greeting",
            vec![PromptArgument {
                name: "name".into(),
                description: Some("Name to greet".into()),
                required: Some(true),
            }],
            |args| async move {
                let name = args
                    .and_then(|a| a.get("name")?.as_str().map(String::from))
                    .unwrap_or_else(|| "World".into());
                Ok(PromptResult::user(format!("Hello, {}! How can I help?", name)))
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
| `with_tool(name, desc, handler)` | Register typed tool |
| `with_raw_tool(name, desc, handler)` | Register untyped tool |
| `with_resource(uri, name, desc, handler)` | Register static resource |
| `with_resource_template(uri, name, desc, handler)` | Register resource template |
| `with_prompt(name, desc, args, handler)` | Register prompt |
| `with_simple_prompt(name, desc, handler)` | Register prompt without args |
| `build()` | Build the server |

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
