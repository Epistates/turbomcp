# WASM & Edge Computing

TurboMCP v3 introduces full WebAssembly support, enabling MCP clients to run in browsers and edge computing environments.

## Overview

The `turbomcp-wasm` crate provides:

- **Browser Support** - Full MCP client using Fetch API and WebSocket
- **Type-Safe** - All MCP types available in JavaScript/TypeScript
- **Async/Await** - Modern Promise-based API
- **Small Binary** - Optimized for minimal bundle size (~50-200KB)

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
