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

Add Bearer token authentication.

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

### McpError

```typescript
class McpError extends Error {
    code: number;
    message: string;
    data?: object;
}
```

### Error Codes

| Code | Description |
|------|-------------|
| -32700 | Parse error |
| -32600 | Invalid request |
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |

### Error Handling Example

```javascript
import { McpClient, McpError } from 'turbomcp-wasm';

try {
    const result = await client.callTool("unknown_tool", {});
} catch (error) {
    if (error instanceof McpError) {
        console.error(`MCP Error [${error.code}]: ${error.message}`);
        if (error.data) {
            console.error("Details:", error.data);
        }
    } else {
        console.error("Network error:", error);
    }
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
                const c = new McpClient(url);
                if (token) c.withAuth(token);
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

The `wasm-server` feature provides server-side MCP implementation for edge platforms.

### Installation

```toml
[dependencies]
turbomcp-wasm = { version = "3.0", default-features = false, features = ["wasm-server"] }
worker = "0.7"
serde = { version = "1.0", features = ["derive"] }
schemars = "1.0"
```

### McpServer

The main server struct that handles incoming MCP requests.

#### builder

```rust
McpServer::builder(name: impl Into<String>, version: impl Into<String>) -> McpServerBuilder
```

Create a new server builder.

```rust
let server = McpServer::builder("my-server", "1.0.0")
    .build();
```

#### handle

```rust
async fn handle(&self, req: worker::Request) -> worker::Result<worker::Response>
```

Handle an incoming Cloudflare Worker request.

```rust
server.handle(req).await
```

### McpServerBuilder

Builder for configuring and creating an MCP server.

#### description

```rust
fn description(self, description: impl Into<String>) -> Self
```

Set the server description shown to clients.

#### instructions

```rust
fn instructions(self, instructions: impl Into<String>) -> Self
```

Set server instructions shown to clients.

#### with_tool

```rust
fn with_tool<A, F, Fut>(
    self,
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> Self
where
    A: DeserializeOwned + JsonSchema + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<ToolResult, String>> + Send + 'static,
```

Register a tool with typed arguments. The argument type must implement `JsonSchema` for automatic schema generation.

```rust
#[derive(Deserialize, JsonSchema)]
struct AddArgs { a: i64, b: i64 }

.with_tool("add", "Add two numbers", |args: AddArgs| async move {
    Ok(ToolResult::text(format!("{}", args.a + args.b)))
})
```

#### with_raw_tool

```rust
fn with_raw_tool<F, Fut>(
    self,
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> Self
```

Register a tool with raw JSON arguments (no schema validation).

#### with_resource

```rust
fn with_resource<F, Fut>(
    self,
    uri: impl Into<String>,
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> Self
```

Register a static resource.

```rust
.with_resource(
    "config://settings",
    "Settings",
    "App settings",
    |uri| async move {
        Ok(ResourceResult::text(&uri, "config data"))
    },
)
```

#### with_resource_template

```rust
fn with_resource_template<F, Fut>(
    self,
    uri_template: impl Into<String>,
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> Self
```

Register a dynamic resource template.

```rust
.with_resource_template(
    "user://{id}",
    "User",
    "User by ID",
    |uri| async move {
        let id = uri.split('/').last().unwrap_or("0");
        Ok(ResourceResult::text(&uri, format!("User {}", id)))
    },
)
```

#### with_prompt

```rust
fn with_prompt<F, Fut>(
    self,
    name: impl Into<String>,
    description: impl Into<String>,
    arguments: Vec<PromptArgument>,
    handler: F,
) -> Self
```

Register a prompt with arguments.

#### with_simple_prompt

```rust
fn with_simple_prompt<F, Fut>(
    self,
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> Self
```

Register a prompt without arguments.

```rust
.with_simple_prompt("help", "Get help", || async move {
    Ok(PromptResult::user("How can I help?"))
})
```

### ToolResult

Result type for tool handlers.

| Method | Description |
|--------|-------------|
| `text(text)` | Create text result |
| `json(value)` | Create JSON result |
| `error(message)` | Create error result |
| `image(data, mime_type)` | Create image result |
| `contents(vec)` | Create multi-content result |

### ResourceResult

Result type for resource handlers.

| Method | Description |
|--------|-------------|
| `text(uri, content)` | Create text resource |
| `json(uri, value)` | Create JSON resource |
| `binary(uri, data, mime_type)` | Create binary resource |

### PromptResult

Result type for prompt handlers.

| Method | Description |
|--------|-------------|
| `user(text)` | Create user message |
| `assistant(text)` | Create assistant message |
| `messages(vec)` | Create multi-message prompt |
| `with_description(text)` | Add description |
| `add_user(text)` | Append user message |
| `add_assistant(text)` | Append assistant message |

## Next Steps

- **[WASM & Edge Guide](../guide/wasm.md)** - Usage patterns
- **[Deployment](../deployment/edge.md)** - Edge deployment
- **[Core Types](core.md)** - MCP type definitions
