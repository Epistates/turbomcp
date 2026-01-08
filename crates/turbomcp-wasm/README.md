# turbomcp-wasm

WebAssembly bindings for TurboMCP - MCP client for browsers and WASI environments.

## Features

- **Browser Support**: Full MCP client using Fetch API and WebSocket
- **Type-Safe**: All MCP types available in JavaScript/TypeScript
- **Async/Await**: Modern Promise-based API
- **Small Binary**: Optimized for minimal bundle size (~50-200KB)

## Installation

### NPM (coming soon)

```bash
npm install turbomcp-wasm
```

### From Source

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for browser
wasm-pack build --target web

# Build for bundler (webpack, etc.)
wasm-pack build --target bundler
```

## Usage

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

```javascript
// webpack.config.js
module.exports = {
  experiments: {
    asyncWebAssembly: true,
  },
};

// app.js
import { McpClient } from 'turbomcp-wasm';

const client = new McpClient("https://api.example.com/mcp");
```

## API Reference

### McpClient

#### Constructor

```typescript
new McpClient(baseUrl: string): McpClient
```

#### Methods

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

## Binary Size

| Configuration | Size |
|--------------|------|
| Core types only | ~50KB |
| + JSON serialization | ~100KB |
| + HTTP client | ~200KB |

## Browser Compatibility

- Chrome 89+
- Firefox 89+
- Safari 15+
- Edge 89+

Requires support for:
- WebAssembly
- Fetch API
- ES2020 (async/await)

## WASI Support (Planned)

WASI Preview 2 support is planned for running in server-side WASM runtimes:
- Wasmtime 29+
- WasmEdge
- Wasmer

## License

MIT
