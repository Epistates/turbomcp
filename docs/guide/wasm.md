# WASM & Edge Computing

TurboMCP v3 introduces full WebAssembly support, enabling both MCP clients and servers to run in browsers and edge computing environments.

## Overview

The `turbomcp-wasm` crate provides:

### Client Features (browser, wasi)

- **Browser Support** - Full MCP client using Fetch API and WebSocket
- **Type-Safe** - All MCP types available in JavaScript/TypeScript
- **Async/Await** - Modern Promise-based API
- **Small Binary** - Optimized for minimal bundle size (~50-200KB)

### Server Features (wasm-server)

- **Edge MCP Servers** - Build full MCP servers running on Cloudflare Workers
- **Type-Safe Handlers** - Automatic JSON schema generation from Rust types
- **Zero Tokio** - Uses wasm-bindgen-futures, no tokio runtime needed
- **Full Protocol** - Tools, resources, prompts, and all standard MCP methods

## Write Once, Run Everywhere

TurboMCP v3 enables true cross-platform MCP servers through the unified `McpHandler` trait. Write your business logic once and deploy it to both native servers (TCP, HTTP, WebSocket) and WASM environments (Cloudflare Workers, Deno Deploy).

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Your McpHandler Implementation               │
│                     (Shared Business Logic)                     │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌─────────────────────────┐     ┌─────────────────────────┐
│     Native Runtime      │     │     WASM Runtime        │
│  ───────────────────    │     │  ───────────────────    │
│  • .run_tcp()           │     │  • WasmHandlerExt       │
│  • .run_http()          │     │  • .handle_worker_      │
│  • .run_websocket()     │     │      request()          │
│  • .serve() (stdio)     │     │  • Cloudflare Workers   │
└─────────────────────────┘     └─────────────────────────┘
```

### The McpHandler Trait

The `McpHandler` trait from `turbomcp-core` defines the unified interface:

```rust
use turbomcp_core::handler::McpHandler;
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_types::*;
use core::future::Future;
use serde_json::Value;

#[derive(Clone)]
struct MyServer {
    greeting: String,
}

impl McpHandler for MyServer {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("my-server", "1.0.0")
            .with_description("A portable MCP server")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            Tool::new("greet", "Say hello to someone"),
            Tool::new("add", "Add two numbers"),
        ]
    }

    fn list_resources(&self) -> Vec<Resource> {
        vec![Resource::new("config://app", "App Config")]
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        vec![Prompt::new("greeting", "A friendly greeting")]
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + 'a {
        let name = name.to_string();
        let greeting = self.greeting.clone();
        async move {
            match name.as_str() {
                "greet" => {
                    let who = args.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("World");
                    Ok(ToolResult::text(format!("{}, {}!", greeting, who)))
                }
                "add" => {
                    let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                    let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                    Ok(ToolResult::text(format!("{}", a + b)))
                }
                _ => Err(McpError::tool_not_found(&name)),
            }
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + 'a {
        let uri = uri.to_string();
        async move {
            match uri.as_str() {
                "config://app" => Ok(ResourceResult::text(&uri, r#"{"debug": true}"#)),
                _ => Err(McpError::resource_not_found(&uri)),
            }
        }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        _args: Option<Value>,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + 'a {
        let name = name.to_string();
        async move {
            match name.as_str() {
                "greeting" => Ok(PromptResult::user("Hello! How can I help you today?")),
                _ => Err(McpError::prompt_not_found(&name)),
            }
        }
    }
}
```

### Native Deployment

Use standard TurboMCP transport methods:

```rust
// main.rs (native binary)
use turbomcp::prelude::*;

mod handler; // Your McpHandler implementation

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = handler::MyServer {
        greeting: "Hello".into()
    };

    // Choose your transport
    server.run_tcp("0.0.0.0:3000").await?;
    // Or: server.run_http("0.0.0.0:8080").await?;
    // Or: server.run_websocket("0.0.0.0:9000").await?;
    // Or: server.serve().await?; // stdio

    Ok(())
}
```

### WASM Deployment

Use `WasmHandlerExt` to run the same handler in WASM:

```rust
// worker.rs (Cloudflare Worker)
use turbomcp_wasm::wasm_server::WasmHandlerExt;
use worker::*;

mod handler; // Same McpHandler implementation!

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = handler::MyServer {
        greeting: "Hello".into()
    };

    // WasmHandlerExt provides handle_worker_request()
    server.handle_worker_request(req).await
}
```

### Project Structure

A typical portable MCP server project:

```
my-mcp-server/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Shared McpHandler implementation
│   └── handler.rs      # Handler logic
├── native/
│   ├── Cargo.toml      # Native binary dependencies
│   └── src/
│       └── main.rs     # Native entry point
└── worker/
    ├── Cargo.toml      # WASM/Worker dependencies
    ├── wrangler.toml   # Cloudflare config
    └── src/
        └── lib.rs      # Worker entry point
```

**Shared library (`src/lib.rs`):**

```rust
pub mod handler;
pub use handler::MyServer;
```

**Native entry (`native/src/main.rs`):**

```rust
use my_mcp_server::MyServer;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() {
    MyServer::default().run_tcp("0.0.0.0:3000").await.unwrap();
}
```

**Worker entry (`worker/src/lib.rs`):**

```rust
use my_mcp_server::MyServer;
use turbomcp_wasm::wasm_server::WasmHandlerExt;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    MyServer::default().handle_worker_request(req).await
}
```

### Benefits

| Aspect | Benefit |
|--------|---------|
| **Single Source of Truth** | Business logic written once, tested once |
| **Type Safety** | Same Rust types across all platforms |
| **Consistent Behavior** | Shared router ensures identical MCP protocol handling |
| **Easy Testing** | Test handler logic in native Rust, deploy everywhere |
| **Gradual Migration** | Start native, add WASM deployment without code changes |

### When to Use Each Approach

| Approach | Best For |
|----------|----------|
| **McpHandler + WasmHandlerExt** | Portable servers, shared business logic |
| **McpServer Builder** | WASM-only servers, quick prototypes |
| **#[server] macro** | Native-focused with macro convenience |
| **#[wasm_server] macro** | WASM-focused with macro convenience |

## Installation

### From NPM

```bash
npm install turbomcp-wasm
```

### From Source

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for browser
wasm-pack build --target web crates/turbomcp-wasm

# Build for bundler (webpack, etc.)
wasm-pack build --target bundler crates/turbomcp-wasm
```

## Browser Usage

### ES Modules

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

  // List resources
  const resources = await client.listResources();
  for (const resource of resources) {
    console.log(`Resource: ${resource.name} (${resource.uri})`);
  }

  // Read a resource
  const content = await client.readResource("file:///example.txt");
  console.log("Content:", content);

  // List and use prompts
  const prompts = await client.listPrompts();
  const promptResult = await client.getPrompt("greeting", { name: "World" });
  console.log("Prompt messages:", promptResult.messages);
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

  // Type-safe tool calls
  interface MyToolArgs {
    query: string;
    limit?: number;
  }

  const result = await client.callTool("search", {
    query: "example",
    limit: 10
  } as MyToolArgs);
}
```

### With Bundler (Webpack/Vite)

**webpack.config.js:**

```javascript
module.exports = {
  experiments: {
    asyncWebAssembly: true,
  },
};
```

**vite.config.js:**

```javascript
import { defineConfig } from 'vite';
import wasm from 'vite-plugin-wasm';

export default defineConfig({
  plugins: [wasm()],
});
```

**App code:**

```javascript
import { McpClient } from 'turbomcp-wasm';

const client = new McpClient("https://api.example.com/mcp");
```

## API Reference

### McpClient

#### Constructor

```typescript
new McpClient(baseUrl: string): McpClient
```

#### Configuration Methods

| Method | Description |
|--------|-------------|
| `withAuth(token: string)` | Add Bearer token authentication |
| `withHeader(key: string, value: string)` | Add custom header |
| `withTimeout(ms: number)` | Set request timeout |

#### Session Methods

| Method | Description |
|--------|-------------|
| `initialize()` | Initialize MCP session |
| `isInitialized()` | Check if session is initialized |
| `getServerInfo()` | Get server implementation info |
| `getServerCapabilities()` | Get server capabilities |
| `ping()` | Ping the server |

#### Tool Methods

| Method | Description |
|--------|-------------|
| `listTools()` | List available tools |
| `callTool(name: string, args?: object)` | Call a tool |

#### Resource Methods

| Method | Description |
|--------|-------------|
| `listResources()` | List available resources |
| `readResource(uri: string)` | Read a resource |
| `listResourceTemplates()` | List resource templates |

#### Prompt Methods

| Method | Description |
|--------|-------------|
| `listPrompts()` | List available prompts |
| `getPrompt(name: string, args?: object)` | Get a prompt |

## WASI Support

TurboMCP v3 includes WASI Preview 2 support for running in server-side WASM runtimes.

### Supported Runtimes

- **Wasmtime 29+**
- **WasmEdge**
- **Wasmer**

### Building for WASI

```bash
# Add WASI target
rustup target add wasm32-wasip2

# Build WASI module
cargo build --target wasm32-wasip2 -p turbomcp-wasm --features wasi
```

### WASI Transports

**StdioTransport** - MCP over STDIO using `wasi:cli/stdin` and `wasi:cli/stdout`:

```rust
use turbomcp_wasm::wasi::StdioTransport;

let transport = StdioTransport::new();
let client = McpClient::new(transport);
```

**HttpTransport** - HTTP-based MCP using `wasi:http/outgoing-handler`:

```rust
use turbomcp_wasm::wasi::HttpTransport;

let transport = HttpTransport::new("https://api.example.com/mcp");
let client = McpClient::new(transport);
```

## no_std Core

The `turbomcp-core` crate provides `no_std` compatible core types:

```toml
[dependencies]
turbomcp-core = { version = "3.0", default-features = false }
```

This enables:

- Embedded systems
- Custom WASM environments
- Minimal runtime footprint

## Binary Size Optimization

| Configuration | Size |
|--------------|------|
| Core types only | ~50KB |
| + JSON serialization | ~100KB |
| + HTTP client | ~200KB |

### Optimization Tips

1. **Use `wasm-opt`**:
```bash
wasm-opt -Os -o optimized.wasm output.wasm
```

2. **Enable LTO**:
```toml
[profile.release]
lto = true
```

3. **Strip debug info**:
```toml
[profile.release]
strip = true
```

## Browser Compatibility

| Browser | Minimum Version |
|---------|-----------------|
| Chrome | 89+ |
| Firefox | 89+ |
| Safari | 15+ |
| Edge | 89+ |

Required browser features:

- WebAssembly
- Fetch API
- ES2020 (async/await)

## Edge Deployment

### Cloudflare Workers

```javascript
import { McpClient } from 'turbomcp-wasm';

export default {
  async fetch(request) {
    const client = new McpClient("https://backend.example.com/mcp");
    await client.initialize();

    const tools = await client.listTools();
    return new Response(JSON.stringify(tools));
  }
};
```

### Vercel Edge Functions

```typescript
import { McpClient } from 'turbomcp-wasm';

export const config = { runtime: 'edge' };

export default async function handler(req: Request) {
  const client = new McpClient("https://backend.example.com/mcp");
  await client.initialize();

  const result = await client.callTool("process", { data: "input" });
  return new Response(JSON.stringify(result));
}
```

### Deno Deploy

```typescript
import init, { McpClient } from 'npm:turbomcp-wasm';

await init();

Deno.serve(async () => {
  const client = new McpClient("https://backend.example.com/mcp");
  await client.initialize();

  const tools = await client.listTools();
  return new Response(JSON.stringify(tools));
});
```

## Error Handling

```javascript
import { McpClient, McpError } from 'turbomcp-wasm';

try {
  const client = new McpClient("https://api.example.com/mcp");
  await client.initialize();
  const result = await client.callTool("my_tool", {});
} catch (error) {
  if (error instanceof McpError) {
    console.error(`MCP Error [${error.code}]: ${error.message}`);
  } else {
    console.error("Network error:", error);
  }
}
```

## Building MCP Servers (wasm-server)

The `wasm-server` feature enables building full MCP servers that run on edge platforms like Cloudflare Workers.

### Installation

```toml
[dependencies]
turbomcp-wasm = { version = "3.0", default-features = false, features = ["wasm-server"] }
worker = "0.7"
serde = { version = "1.0", features = ["derive"] }
schemars = "1.0"
getrandom = { version = "0.3", features = ["wasm_js"] }
```

### Basic Server (Builder API)

```rust
use turbomcp_wasm::wasm_server::{McpServer, ToolResult};
use worker::*;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {
    name: String,
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-mcp-server", "1.0.0")
        .description("My MCP server on the edge")
        .tool("hello", "Say hello", |args: HelloArgs| async move {
            format!("Hello, {}!", args.name)  // IntoToolResponse - returns any type!
        })
        .build();

    server.handle(req).await
}
```

### Zero-Boilerplate Server (Macros)

With the `macros` feature, you can define MCP servers with minimal code using procedural macros:

```toml
[dependencies]
turbomcp-wasm = { version = "3.0", default-features = false, features = ["macros"] }
worker = "0.7"
serde = { version = "1.0", features = ["derive"] }
schemars = "1.0"
```

```rust
use turbomcp_wasm::prelude::*;
use serde::Deserialize;

#[derive(Clone)]
struct MyServer {
    greeting: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct GreetArgs {
    name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs {
    a: i64,
    b: i64,
}

#[server(name = "my-server", version = "1.0.0", description = "My MCP server")]
impl MyServer {
    #[tool("Greet someone by name")]
    async fn greet(&self, args: GreetArgs) -> String {
        format!("{}, {}!", self.greeting, args.name)
    }

    #[tool("Add two numbers")]
    async fn add(&self, args: AddArgs) -> i64 {
        args.a + args.b
    }

    #[tool("Get server status")]
    async fn status(&self) -> String {
        "Server is running".to_string()
    }

    #[resource("config://app")]
    async fn config(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, r#"{"theme": "dark"}"#)
    }

    #[prompt("Default greeting")]
    async fn greeting_prompt(&self) -> PromptResult {
        PromptResult::user("Hello! How can I help?")
    }
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = MyServer { greeting: "Hello".into() };
    server.into_mcp_server().handle(req).await
}
```

The macros generate the same efficient code as the builder API, but with cleaner syntax.

### Prelude Module

For convenience, import everything you need with the prelude:

```rust
use turbomcp_wasm::prelude::*;

// This imports:
// - McpServer, McpServerBuilder
// - ToolResult, ToolError, ResourceResult, PromptResult
// - IntoToolResponse, Text, Json, Image
// - #[server], #[tool], #[resource], #[prompt] macros (with "macros" feature)
```

### Ergonomic Handler System (IntoToolResponse)

The new `IntoToolResponse` trait provides axum-inspired ergonomics for tool handlers. Return any type that implements the trait:

```rust
// String - automatically converted to text content
.tool("greet", "Say hello", |args: GreetArgs| async move {
    format!("Hello, {}!", args.name)
})

// Numbers - converted to text
.tool("add", "Add numbers", |args: AddArgs| async move {
    args.a + args.b
})

// Explicit text wrapper
.tool("text_example", "Return text", |_: NoArgs| async move {
    Text("Explicit text content".to_string())
})

// JSON serialization
.tool("json_example", "Return JSON", |_: NoArgs| async move {
    Json(serde_json::json!({"key": "value"}))
})

// Image response
.tool("image", "Return image", |_: NoArgs| async move {
    Image {
        data: base64_data,
        mime_type: "image/png".to_string(),
    }
})

// Result types for error handling
.tool("fallible", "Might fail", |args: Args| async move {
    if args.value < 0 {
        Err(ToolError::new("Value must be positive"))
    } else {
        Ok(format!("Value: {}", args.value))
    }
})

// ToolResult for full control
.tool("full_control", "Multiple content items", |_: NoArgs| async move {
    ToolResult::contents(vec![
        Content::text("First item"),
        Content::text("Second item"),
    ])
})
```

### Tools Without Arguments

Use `tool_no_args` for tools that don't need input:

```rust
.tool_no_args("status", "Get server status", || async move {
    "Server is running"
})
```

### Tool Results

```rust
// Text result
ToolResult::text("Hello, World!")

// JSON result
ToolResult::json(&my_struct)?

// Error result
ToolResult::error("Something went wrong")

// Image result (base64)
ToolResult::image(base64_data, "image/png")

// Multiple content items
ToolResult::contents(vec![
    Content::Text { text: "Text".into(), annotations: None },
    Content::Image { data: b64, mime_type: "image/png".into(), annotations: None },
])
```

### Resources

```rust
// Static resource
.resource(
    "config://settings",
    "Settings",
    "Application settings",
    |uri: String| async move {
        ResourceResult::json(&uri, &settings)
    },
)

// Dynamic resource template
.resource_template(
    "user://{id}",
    "User Profile",
    "Get user by ID",
    |uri: String| async move {
        let id = uri.split('/').last().unwrap_or("0");
        ResourceResult::text(&uri, format!("User {}", id))
    },
)
```

### Prompts

```rust
use turbomcp_core::types::prompts::PromptArgument;

// Prompt with arguments
.prompt(
    "greeting",
    "Generate a greeting",
    |args: Option<GreetingArgs>| async move {
        let name = args.map(|a| a.name).unwrap_or_else(|| "World".into());
        PromptResult::user(format!("Hello, {}!", name))
    },
)

// Simple prompt (no arguments)
.prompt_no_args("help", "Get help", || async move {
    PromptResult::user("How can I help you today?")
})
```

### Building and Deploying

```bash
# Build for Cloudflare Workers
wrangler dev

# Or build manually
cargo build --target wasm32-unknown-unknown --release
```

## React Integration

```tsx
import { useState, useEffect } from 'react';
import init, { McpClient } from 'turbomcp-wasm';

function useMcpClient(url: string) {
  const [client, setClient] = useState<McpClient | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    async function initClient() {
      try {
        await init();
        const c = new McpClient(url);
        await c.initialize();
        setClient(c);
      } catch (e) {
        setError(e as Error);
      } finally {
        setLoading(false);
      }
    }
    initClient();
  }, [url]);

  return { client, loading, error };
}

function ToolList() {
  const { client, loading, error } = useMcpClient("https://api.example.com/mcp");
  const [tools, setTools] = useState([]);

  useEffect(() => {
    if (client) {
      client.listTools().then(setTools);
    }
  }, [client]);

  if (loading) return <div>Loading...</div>;
  if (error) return <div>Error: {error.message}</div>;

  return (
    <ul>
      {tools.map(tool => (
        <li key={tool.name}>{tool.name}: {tool.description}</li>
      ))}
    </ul>
  );
}
```

## Next Steps

- **[Wire Codecs](wire-codecs.md)** - Serialization formats
- **[Tower Middleware](tower-middleware.md)** - Composable middleware
- **[Deployment](../deployment/edge.md)** - Edge deployment guide
- **[API Reference](../api/wasm.md)** - Full WASM API
