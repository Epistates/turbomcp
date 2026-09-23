# WASM Bindings API Reference

The `turbomcp-wasm` crate provides WebAssembly bindings for TurboMCP, enabling MCP clients and servers in browsers and edge environments.

## Overview

WASM bindings provide:

### Client Features

- **Browser Support** - Full MCP client using Fetch API
- **TypeScript Types** - Complete type definitions
- **Async/Await** - Promise-based API
- **Small Binary** - Optimized for bundle size (~50-200KB)

### Server Features (wasm-server)

- **Edge MCP Servers** - Build servers on Cloudflare Workers
- **Type-Safe Handlers** - Automatic JSON schema from Rust types
- **Zero Tokio** - Uses wasm-bindgen-futures for async
- **Full Protocol** - Tools, resources, prompts support

## Installation

### NPM

```bash
npm install turbomcp-wasm
```

### From Source

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for browser (ES modules)
wasm-pack build --target web crates/turbomcp-wasm

# Build for bundler
wasm-pack build --target bundler crates/turbomcp-wasm

# Build for Node.js
wasm-pack build --target nodejs crates/turbomcp-wasm
```

## McpClient Class

### Constructor

```typescript
new McpClient(baseUrl: string): McpClient
```

Creates a new MCP client connected to the specified server URL.

```javascript
import init, { McpClient } from 'turbomcp-wasm';

await init();
const client = new McpClient("https://api.example.com/mcp");
```

### Configuration Methods

#### withAuth

```typescript
withAuth(token: string): McpClient
```

Add Bearer token authentication. Like `withHeader` and `withTimeout`, it
consumes the client and returns a configured one, so always use the returned
value; the original object can no longer be called.

```javascript
const client = new McpClient(url)
    .withAuth("your-api-token");
```

#### withHeader

```typescript
withHeader(key: string, value: string): McpClient
```

Add a custom HTTP header.

```javascript
const client = new McpClient(url)
    .withHeader("X-Custom-Header", "value");
```

#### withTimeout

```typescript
withTimeout(ms: number): McpClient
```

Set request timeout in milliseconds.

```javascript
const client = new McpClient(url)
    .withTimeout(30000);  // 30 seconds
```

### Session Methods

#### initialize

```typescript
initialize(): Promise<InitializeResult>
```

Initialize the MCP session. Must be called before other operations.

```javascript
const result = await client.initialize();
console.log("Server:", result.serverInfo.name);
console.log("Version:", result.serverInfo.version);
console.log("Capabilities:", result.capabilities);
```

#### isInitialized

```typescript
isInitialized(): boolean
```

Check if the session is initialized.

```javascript
if (!client.isInitialized()) {
    await client.initialize();
}
```

#### getServerInfo

```typescript
getServerInfo(): ServerInfo | null
```

Get server implementation info after initialization.

```javascript
const info = client.getServerInfo();
console.log(`${info.name} v${info.version}`);
```

#### getServerCapabilities

```typescript
getServerCapabilities(): ServerCapabilities | null
```

Get server capabilities after initialization.

```javascript
const caps = client.getServerCapabilities();
if (caps.tools) {
    console.log("Server supports tools");
}
```

#### ping

```typescript
ping(): Promise<void>
```

Ping the server to check connectivity.

```javascript
await client.ping();
console.log("Server is alive");
```

### Tool Methods

#### listTools

```typescript
listTools(): Promise<Tool[]>
```

List all available tools.

```javascript
const tools = await client.listTools();
for (const tool of tools) {
    console.log(`${tool.name}: ${tool.description}`);
}
```

#### callTool

```typescript
callTool(name: string, args?: object): Promise<CallToolResult>
```

Call a tool with optional arguments.

```javascript
const result = await client.callTool("calculator", {
    expression: "2 + 2"
});

for (const content of result.content) {
    if (content.type === "text") {
        console.log("Result:", content.text);
    }
}
```

### Resource Methods

#### listResources

```typescript
listResources(): Promise<Resource[]>
```

List all available resources.

```javascript
const resources = await client.listResources();
for (const resource of resources) {
    console.log(`${resource.name} (${resource.uri})`);
}
```

#### readResource

```typescript
readResource(uri: string): Promise<ReadResourceResult>
```

Read a resource by URI.

```javascript
const result = await client.readResource("file:///data.json");
for (const content of result.contents) {
    if (content.text) {
        console.log("Content:", content.text);
    }
}
```

#### listResourceTemplates

```typescript
listResourceTemplates(): Promise<ResourceTemplate[]>
```

List resource URI templates.

```javascript
const templates = await client.listResourceTemplates();
for (const template of templates) {
    console.log(`${template.name}: ${template.uriTemplate}`);
}
```

### Prompt Methods

#### listPrompts

```typescript
listPrompts(): Promise<Prompt[]>
```

List all available prompts.

```javascript
const prompts = await client.listPrompts();
for (const prompt of prompts) {
    console.log(`${prompt.name}: ${prompt.description}`);
}
```

#### getPrompt

```typescript
getPrompt(name: string, args?: object): Promise<GetPromptResult>
```

Get a prompt with optional arguments.

```javascript
const result = await client.getPrompt("greeting", {
    name: "World"
});

for (const message of result.messages) {
    console.log(`${message.role}: ${message.content.text}`);
}
```

## TypeScript Types

### Tool

```typescript
interface Tool {
    name: string;
    description?: string;
    inputSchema: object;
    annotations?: object;
}
```

### Resource

```typescript
interface Resource {
    uri: string;
    name: string;
    description?: string;
    mimeType?: string;
    annotations?: object;
}
```

### Prompt

```typescript
interface Prompt {
    name: string;
    description?: string;
    arguments?: PromptArgument[];
}

interface PromptArgument {
    name: string;
    description?: string;
    required?: boolean;
}
```

### Content

```typescript
type Content = TextContent | ImageContent | EmbeddedResource;

interface TextContent {
    type: "text";
    text: string;
    annotations?: object;
}

interface ImageContent {
    type: "image";
    data: string;  // base64
    mimeType: string;
    annotations?: object;
}

interface EmbeddedResource {
    type: "resource";
    resource: ResourceContents;
    annotations?: object;
}
```

### ServerInfo

```typescript
interface ServerInfo {
    name: string;
    version: string;
}
```

### ServerCapabilities

```typescript
interface ServerCapabilities {
    tools?: { listChanged?: boolean };
    resources?: { subscribe?: boolean; listChanged?: boolean };
    prompts?: { listChanged?: boolean };
    logging?: object;
    experimental?: object;
}
```

### InitializeResult

```typescript
interface InitializeResult {
    protocolVersion: string;
    capabilities: ServerCapabilities;
    serverInfo: ServerInfo;
    instructions?: string;
}
```

### CallToolResult

```typescript
interface CallToolResult {
    content: Content[];
    isError?: boolean;
}
```

### ReadResourceResult

```typescript
interface ReadResourceResult {
    contents: ResourceContents[];
}

interface ResourceContents {
    uri: string;
    mimeType?: string;
    text?: string;
    blob?: Uint8Array;
}
```

### GetPromptResult

```typescript
interface GetPromptResult {
    description?: string;
    messages: PromptMessage[];
}

interface PromptMessage {
    role: "user" | "assistant";
    content: TextContent | ImageContent | EmbeddedResource;
}
```

## Error Handling

### Rejections

A failed call rejects its promise with a string describing the error: the
server's JSON-RPC error (for example `Method not found: ...` for code
`-32601`), a transport failure, or a response that could not be parsed. There
is no error class to test with `instanceof`.

A tool that ran and failed is not a rejection: the promise resolves with a
`CallToolResult` whose `isError` is `true`.

### Error Handling Example

```javascript
try {
    const result = await client.callTool("my_tool", {});
    if (result.isError) {
        console.error("Tool failed:", result.content);
    }
} catch (error) {
    // A string: JSON-RPC error, network failure, or parse failure
    console.error("MCP request failed:", error);
}
```

## Usage Examples

### Basic Usage

```javascript
import init, { McpClient } from 'turbomcp-wasm';

async function main() {
    await init();

    const client = new McpClient("https://api.example.com/mcp")
        .withAuth("token")
        .withTimeout(30000);

    await client.initialize();

    const tools = await client.listTools();
    console.log("Tools:", tools);

    const result = await client.callTool("hello", { name: "World" });
    console.log("Result:", result);
}

main().catch(console.error);
```

### React Hook

```typescript
import { useState, useEffect } from 'react';
import init, { McpClient } from 'turbomcp-wasm';

export function useMcpClient(url: string, token?: string) {
    const [client, setClient] = useState<McpClient | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<Error | null>(null);

    useEffect(() => {
        async function initClient() {
            try {
                await init();
                // withAuth consumes the client and returns a new one: use its result
                let c = new McpClient(url);
                if (token) c = c.withAuth(token);
                await c.initialize();
                setClient(c);
            } catch (e) {
                setError(e as Error);
            } finally {
                setLoading(false);
            }
        }
        initClient();
    }, [url, token]);

    return { client, loading, error };
}
```

### Vue Composable

```typescript
import { ref, onMounted } from 'vue';
import init, { McpClient } from 'turbomcp-wasm';

export function useMcpClient(url: string) {
    const client = ref<McpClient | null>(null);
    const loading = ref(true);
    const error = ref<Error | null>(null);

    onMounted(async () => {
        try {
            await init();
            client.value = new McpClient(url);
            await client.value.initialize();
        } catch (e) {
            error.value = e as Error;
        } finally {
            loading.value = false;
        }
    });

    return { client, loading, error };
}
```

## Binary Size

| Configuration | Size (gzipped) |
|--------------|----------------|
| Core only | ~20KB |
| + JSON | ~40KB |
| + HTTP client | ~80KB |
| Full | ~100KB |

### Size Optimization

```bash
# Optimize with wasm-opt
wasm-opt -Os -o optimized.wasm pkg/turbomcp_wasm_bg.wasm
```

## Browser Compatibility

| Browser | Minimum Version |
|---------|-----------------|
| Chrome | 89+ |
| Firefox | 89+ |
| Safari | 15+ |
| Edge | 89+ |

## Server API (wasm-server feature)

The `wasm-server` feature provides server-side MCP implementation for edge
platforms such as Cloudflare Workers. Build for `wasm32-unknown-unknown`.

### Installation

=== "Builder API"
    ```toml
    [dependencies]
    turbomcp-wasm = { version = "3.5.0", default-features = false, features = ["wasm-server"] }
    worker = "0.8"
    serde = { version = "1.0", features = ["derive"] }
    schemars = "1.2"
    ```

=== "Macros (Zero-Boilerplate)"
    ```toml
    [dependencies]
    turbomcp-wasm = { version = "3.5.0", default-features = false, features = ["macros"] }
    worker = "0.8"
    serde = { version = "1.0", features = ["derive"] }
    schemars = "1.2"
    ```

### Prelude Module

The prelude provides convenient imports:

```rust
use turbomcp_wasm::prelude::*;

// Imports:
// - McpServer, McpServerBuilder, WasmHandlerExt
// - ToolResult, ToolError, ResourceResult, PromptResult
// - IntoToolResponse, Text, Json, Image
// - McpError, McpResult, ErrorKind, McpHandler, Tool, Resource, Prompt
// - worker's Request, Response, Env, Context
// - #[server], #[tool], #[resource], #[prompt] macros (with "macros" feature)
```

The prelude does not include worker's `#[event]` macro or its `Result` alias;
import those from `worker`.

### McpServer

The main server struct that handles incoming MCP requests.
`McpServer::builder(name, version)` returns a `McpServerBuilder`, and
`handle(req)` answers a Cloudflare Worker request:

```rust
use turbomcp_wasm::prelude::*;
use worker::{event, Result};

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-server", "1.0.0")
        .description("An edge MCP server")
        .instructions("Call `status` to check the server")
        .tool_no_args("status", "Get server status", || async move { "Server is running" })
        .build();

    server.handle(req).await
}
```

`handle` serves stateless JSON-RPC over POST with the default `EndpointConfig`:
JSON bodies up to 1 MiB, and browser `Origin`s limited to loopback ones.
`McpServer` implements `McpHandler`, so to accept other origins use
`WasmHandlerExt::handle_worker_request_with_config(req, &config)` with an
`EndpointConfig` built by `allow_origin(...)`. The `streamable` feature adds
sessions and SSE.

### McpServerBuilder

Builder for configuring and creating an MCP server. Its methods:

| Method | Handler shape |
|---|---|
| `description(text)`, `instructions(text)` | — |
| `tool(name, description, handler)` | `Fn(A) -> Fut`, `A: Deserialize + JsonSchema`; output any `IntoToolResponse` |
| `tool_no_args(name, description, handler)` | `Fn() -> Fut` |
| `tool_raw(name, description, handler)` | `Fn(serde_json::Value) -> Fut`, no schema |
| `tool_with_ctx`, `tool_with_ctx_no_args`, `tool_with_ctx_raw` | as above, with `Arc<RequestContext>` first |
| `resource(uri, name, description, handler)` | `Fn(String) -> Fut`; output `ResourceResult` or `Result<ResourceResult, E>` |
| `resource_template(uri_template, name, description, handler)` | same, for an RFC 6570 template |
| `resource_with_ctx`, `resource_template_with_ctx` | `Fn(Arc<RequestContext>, String) -> Fut` |
| `prompt(name, description, handler)` | `Fn(Option<A>) -> Fut`, `A: Deserialize + JsonSchema`; output `PromptResult` or `Result<PromptResult, E>` |
| `prompt_no_args(name, description, handler)` | `Fn() -> Fut` |
| `prompt_with_ctx`, `prompt_with_ctx_no_args` | with `Arc<RequestContext>` first |
| `build()` | Produces the `McpServer` |

Handlers must be `Clone` (a closure is, when what it captures is). The
argument type's `JsonSchema` becomes the tool's input schema, and a prompt's
argument struct fields become its prompt arguments. The capabilities
advertised in `initialize` follow what was registered.

```rust
use serde::Deserialize;
use turbomcp_wasm::prelude::*;

#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs {
    a: i64,
    b: i64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct GreetArgs {
    name: String,
}

fn build_server() -> McpServer {
    McpServer::builder("my-server", "1.0.0")
        // Simple return - uses IntoToolResponse
        .tool("add", "Add two numbers", |args: AddArgs| async move { args.a + args.b })
        // Or with explicit ToolResult
        .tool("add_text", "Add two numbers", |args: AddArgs| async move {
            ToolResult::text(format!("{}", args.a + args.b))
        })
        // No arguments
        .tool_no_args("status", "Get server status", || async move { "Server is running" })
        // Raw JSON arguments (no schema validation)
        .tool_raw("echo", "Echo any JSON", |args: serde_json::Value| async move {
            format!("Received: {args}")
        })
        // A static resource
        .resource("config://settings", "Settings", "App settings", |uri: String| async move {
            ResourceResult::text(&uri, "config data")
        })
        // A resource template: the handler receives the full URI
        .resource_template("user://{id}", "User", "User by ID", |uri: String| async move {
            let id = uri.trim_start_matches("user://").to_string();
            ResourceResult::text(&uri, format!("User {id}"))
        })
        // A prompt with optional typed arguments
        .prompt("greeting", "Generate greeting", |args: Option<GreetArgs>| async move {
            let name = args.map(|a| a.name).unwrap_or_else(|| "World".into());
            PromptResult::user(format!("Hello, {name}!"))
        })
        .prompt_no_args("help", "Get help", || async move {
            PromptResult::user("How can I help?")
        })
        .build()
}
```

### ToolResult

Result type for tool handlers (an alias for `CallToolResult`).

| Method | Description |
|--------|-------------|
| `text(text)` | Create text result |
| `json(&value)` | Create a JSON text result; returns `Result<ToolResult, serde_json::Error>` |
| `error(message)` | Create error result (`isError: true`) |
| `image(data, mime_type)` | Create image result (`data` is base64) |
| `contents(vec)` | Create multi-content result |

### ResourceResult

Result type for resource handlers.

| Method | Description |
|--------|-------------|
| `text(uri, content)` | Create text resource |
| `json(uri, &value)` | Create JSON resource; returns `Result<ResourceResult, serde_json::Error>` |
| `binary(uri, base64_data, mime_type)` | Create binary resource from base64-encoded data |

### PromptResult

Result type for prompt handlers.

| Method | Description |
|--------|-------------|
| `user(text)` | Create user message |
| `assistant(text)` | Create assistant message |
| `new(vec)` | Create multi-message prompt from `Vec<Message>` |
| `with_description(text)` | Add description |
| `add_user(text)` | Append user message |
| `add_assistant(text)` | Append assistant message |

### IntoToolResponse Trait

The `IntoToolResponse` trait enables ergonomic handler returns (axum-inspired). Any type implementing this trait can be returned from tool handlers:

| Type | Behavior |
|------|----------|
| `String` | Converted to text content |
| `&str` | Converted to text content |
| Integer and float types | Converted to text (string representation) |
| `bool` | Converted to text (`"true"` or `"false"`) |
| `()` | Empty result |
| `Text(String)` | Explicit text content wrapper |
| `Json<T>` | JSON serialization of value |
| `Image { data, mime_type }` | Base64-encoded image |
| `ToolResult` | Direct tool result (full control) |
| `Result<T, E>` where `E: Into<ToolError>` | Ok → response, Err → error result |
| `Option<T>` | Some → response, None → the text `"No result"` |

**Example:**

```rust
use serde::{Deserialize, Serialize};
use turbomcp_wasm::prelude::*;

#[derive(Deserialize, schemars::JsonSchema)]
struct Args {
    name: String,
    items: Vec<String>,
    valid: bool,
}

#[derive(Clone, Serialize)]
struct Report {
    total: usize,
}

fn build_server() -> McpServer {
    // Return any IntoToolResponse type
    McpServer::builder("demo", "1.0.0")
        .tool("greet", "Greet", |args: Args| async move { format!("Hello, {}!", args.name) })
        .tool("count", "Count", |args: Args| async move { args.items.len() as i64 })
        .tool("data", "Get data", |args: Args| async move {
            Json(Report { total: args.items.len() })
        })
        .tool("fallible", "Might fail", |args: Args| async move {
            if args.valid { Ok("Success") } else { Err(ToolError::new("Invalid")) }
        })
        .build()
}
```

### ToolError

Error type for tool handlers.

```rust
use turbomcp_wasm::prelude::*;

// Create error
let plain = ToolError::new("Something went wrong");

// With code
let coded = ToolError::with_code(-32000, "Custom error");

// From other errors: `From` impls cover McpError, serde_json, io, UTF-8 and
// number-parsing errors, strings, and boxed errors, so `?` works on them
fn parse(input: &str) -> Result<i64, ToolError> {
    Ok(input.parse::<i64>()?)
}

// Anything else that implements Display, with context, via IntoToolError
use turbomcp_wasm::wasm_server::IntoToolError;

fn parse_url(input: &str) -> Result<std::net::IpAddr, ToolError> {
    input.parse().map_err(|e: std::net::AddrParseError| e.tool_err("invalid address"))
}
```

## Procedural Macros (macros feature)

The `macros` feature provides zero-boilerplate server definition. These are
separate from `turbomcp`'s native `#[server]` macro: they generate a builder
call, not an `McpHandler` impl, and have their own rules below.

### #[server]

Transforms an impl block into an MCP server.

```rust
use turbomcp_wasm::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0", description = "Optional description")]
impl MyServer {
    #[tool("Get server status")]
    async fn status(&self) -> String {
        "OK".to_string()
    }
}
```

**Attributes:**

| Attribute | Required | Description |
|-----------|----------|-------------|
| `name` | No | Server name (default: `"mcp-server"`) |
| `version` | No | Server version (default: `"1.0.0"`) |
| `description` | No | Server description |

Values must be string literals. Unlike the native macro, other keys are
ignored rather than rejected.

**Generated Methods:**

| Method | Description |
|--------|-------------|
| `into_mcp_server(self) -> McpServer` | Create MCP server from instance |
| `get_tools_metadata()` | `(name, description, tags, version)` for each tool |
| `get_resources_metadata()` | `(uri_template, name, tags, version)` for each resource |
| `get_prompts_metadata()` | `(name, description, tags, version)` for each prompt |
| `get_tool_tags()`, `get_resource_tags()`, `get_prompt_tags()` | `(name, tags)` for components with tags |
| `server_info() -> (&str, &str)` | Get (name, version) |

### #[tool]

Mark a method as an MCP tool handler. It takes `&self` and, optionally, one
argument struct (`Deserialize + JsonSchema`), which becomes the input schema.
An `Arc<RequestContext>` parameter before it gives the handler the request
context. `#[tool]`, `#[resource]`, and `#[prompt]` accept a description string,
or `description = "..."`, `tags = [...]`, and `version = "..."`.

```rust
use serde::Deserialize;
use turbomcp_wasm::prelude::*;

#[derive(Deserialize, schemars::JsonSchema)]
struct MyArgs {
    query: String,
}

#[derive(Clone)]
struct Tools;

#[server(name = "tools")]
impl Tools {
    #[tool("Description of what this tool does")]
    async fn search(&self, args: MyArgs) -> String {
        format!("results for {}", args.query)
    }

    // Without arguments
    #[tool("Get server status")]
    async fn status(&self) -> String {
        "OK".to_string()
    }
}
```

**Return types:** Any type implementing `IntoToolResponse` (see table above).

### #[resource]

Mark a method as an MCP resource handler. It takes `&self` and the requested
URI, and returns a `ResourceResult` or `Result<ResourceResult, E>`.

```rust
use serde::Serialize;
use turbomcp_wasm::prelude::*;

#[derive(Serialize)]
struct User {
    id: u64,
}

#[derive(Clone)]
struct Resources;

#[server(name = "resources")]
impl Resources {
    #[resource("config://app")]
    async fn config(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, "config data")
    }

    // Template URIs
    #[resource("user://{id}")]
    async fn user(&self, uri: String) -> Result<ResourceResult, serde_json::Error> {
        let id = uri.trim_start_matches("user://").parse().unwrap_or(0);
        ResourceResult::json(&uri, &User { id })
    }
}
```

### #[prompt]

Mark a method as an MCP prompt handler. It takes `&self` and returns a
`PromptResult` or `Result<PromptResult, E>`.

```rust
use turbomcp_wasm::prelude::*;

#[derive(Clone)]
struct Prompts;

#[server(name = "prompts")]
impl Prompts {
    #[prompt("Help prompt")]
    async fn help(&self) -> PromptResult {
        PromptResult::user("How can I help?")
    }
}
```

A prompt method with arguments (`args: Option<Args>`) does not currently
compile: the macro wraps the declared type in a second `Option`. Register
prompts that take arguments with `McpServerBuilder::prompt` instead, as in the
builder example above.

### Complete Macro Example

```rust
use serde::Deserialize;
use turbomcp_wasm::prelude::*;
use worker::event;

#[derive(Clone)]
struct Calculator;

#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs { a: i64, b: i64 }

#[derive(Deserialize, schemars::JsonSchema)]
struct MulArgs { a: i64, b: i64 }

#[server(name = "calculator", version = "2.0.0", description = "Math operations")]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, args: AddArgs) -> i64 {
        args.a + args.b
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, args: MulArgs) -> i64 {
        args.a * args.b
    }

    #[tool("Get calculator info")]
    async fn info(&self) -> String {
        "Calculator v2.0".to_string()
    }

    #[resource("config://calculator")]
    async fn config(&self, uri: String) -> Result<ResourceResult, serde_json::Error> {
        ResourceResult::json(&uri, &serde_json::json!({"precision": 10}))
    }

    #[prompt("Math help")]
    async fn help(&self) -> PromptResult {
        PromptResult::user("I can add and multiply numbers. Try: add 2 3")
    }
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> worker::Result<Response> {
    Calculator.into_mcp_server().handle(req).await
}
```

## Next Steps

- **[WASM & Edge Guide](../guide/wasm.md)** - Usage patterns
- **[Deployment](../deployment/edge.md)** - Edge deployment
- **[Core Types](core.md)** - MCP type definitions
