# Edge & WASM Deployment

Deploy TurboMCP to edge computing platforms and WASM runtimes.

## Overview

TurboMCP v3 supports deployment to:

- **Browser clients** via `turbomcp-wasm`
- **Cloudflare Workers**
- **Vercel Edge Functions**
- **Deno Deploy**
- **WASI runtimes** (Wasmtime, WasmEdge, Wasmer)

## Browser Deployment

### Building for Browser

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for browser (ES modules)
wasm-pack build --target web crates/turbomcp-wasm

# Build for bundler (webpack, vite)
wasm-pack build --target bundler crates/turbomcp-wasm
```

### Basic HTML

```html
<!DOCTYPE html>
<html>
<head>
  <title>MCP Client</title>
</head>
<body>
  <script type="module">
    import init, { McpClient } from './pkg/turbomcp_wasm.js';

    async function main() {
      await init();

      const client = new McpClient("https://api.example.com/mcp");
      await client.initialize();

      const tools = await client.listTools();
      console.log("Tools:", tools);
    }

    main();
  </script>
</body>
</html>
```

### With Vite

**vite.config.js:**

```javascript
import { defineConfig } from 'vite';
import wasm from 'vite-plugin-wasm';
import topLevelAwait from 'vite-plugin-top-level-await';

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
});
```

**main.js:**

```javascript
import init, { McpClient } from 'turbomcp-wasm';

await init();

const client = new McpClient("https://api.example.com/mcp");
await client.initialize();

export { client };
```

### With Webpack

**webpack.config.js:**

```javascript
module.exports = {
  experiments: {
    asyncWebAssembly: true,
    topLevelAwait: true,
  },
  module: {
    rules: [
      {
        test: /\.wasm$/,
        type: 'webassembly/async',
      },
    ],
  },
};
```

## Cloudflare Workers

TurboMCP supports two approaches for Cloudflare Workers:

1. **MCP Client** - JavaScript/WASM client that connects to external MCP servers
2. **MCP Server** - Native Rust server running on Workers edge network

### Native MCP Server (Rust)

Build full MCP servers in Rust that run on Cloudflare Workers using the `wasm-server` feature.

**Cargo.toml:**

```toml
[package]
name = "my-mcp-server"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
turbomcp-wasm = { version = "3.0", default-features = false, features = ["wasm-server"] }
worker = "0.7"
serde = { version = "1.0", features = ["derive"] }
schemars = "1.0"
getrandom = { version = "0.3", features = ["wasm_js"] }
```

**src/lib.rs:**

```rust
use turbomcp_wasm::wasm_server::{McpServer, ToolResult, ResourceResult, PromptResult};
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
    let server = McpServer::builder("my-edge-mcp-server", "1.0.0")
        .description("MCP server running on Cloudflare Workers")
        .instructions("Use the hello and add tools to get started")

        // Register tools with automatic schema generation
        .with_tool("hello", "Say hello to someone", |args: HelloArgs| async move {
            Ok(ToolResult::text(format!("Hello, {}!", args.name)))
        })
        .with_tool("add", "Add two numbers", |args: AddArgs| async move {
            Ok(ToolResult::text(format!("{}", args.a + args.b)))
        })

        // Static resource
        .with_resource(
            "config://settings",
            "Server Settings",
            "Current server configuration",
            |_uri| async move {
                Ok(ResourceResult::json("config://settings", &serde_json::json!({
                    "version": "1.0.0",
                    "environment": "edge"
                }))?)
            },
        )

        .build();

    server.handle(req).await
}
```

**wrangler.toml:**

```toml
name = "my-mcp-server"
main = "build/worker/shim.mjs"
compatibility_date = "2024-01-01"

[build]
command = "cargo install -q worker-build && worker-build --release"
```

**Deploy:**

```bash
wrangler dev     # Local development
wrangler deploy  # Production deployment
```

### MCP Client (JavaScript/WASM)

Use the WASM client to connect to external MCP servers from Workers.

**Setup:**

```bash
npm create cloudflare@latest my-mcp-worker
cd my-mcp-worker
npm install turbomcp-wasm
```

**src/index.js:**

```javascript
import init, { McpClient } from 'turbomcp-wasm';

let initialized = false;

async function ensureInit() {
  if (!initialized) {
    await init();
    initialized = true;
  }
}

export default {
  async fetch(request, env) {
    await ensureInit();

    const client = new McpClient(env.MCP_SERVER_URL)
      .withAuth(env.MCP_API_KEY)
      .withTimeout(25000);  // Workers have 30s limit

    await client.initialize();

    const url = new URL(request.url);

    if (url.pathname === '/tools') {
      const tools = await client.listTools();
      return Response.json(tools);
    }

    if (url.pathname === '/call' && request.method === 'POST') {
      const { tool, args } = await request.json();
      const result = await client.callTool(tool, args);
      return Response.json(result);
    }

    return new Response('Not Found', { status: 404 });
  },
};
```

**wrangler.toml:**

```toml
name = "mcp-proxy"
main = "src/index.js"
compatibility_date = "2024-01-01"

[vars]
MCP_SERVER_URL = "https://backend.example.com/mcp"

# Use Workers KV for caching (optional)
[[kv_namespaces]]
binding = "CACHE"
id = "your-kv-namespace-id"
```

**Deploy:**

```bash
npx wrangler deploy
```

## Vercel Edge Functions

### Setup

```bash
npx create-next-app@latest my-mcp-app
cd my-mcp-app
npm install turbomcp-wasm
```

### Edge Function

**app/api/mcp/route.ts:**

```typescript
import { NextRequest, NextResponse } from 'next/server';
import init, { McpClient } from 'turbomcp-wasm';

export const runtime = 'edge';

let client: McpClient | null = null;

async function getClient() {
  if (!client) {
    await init();
    client = new McpClient(process.env.MCP_SERVER_URL!)
      .withAuth(process.env.MCP_API_KEY!);
    await client.initialize();
  }
  return client;
}

export async function GET(request: NextRequest) {
  const c = await getClient();
  const tools = await c.listTools();
  return NextResponse.json(tools);
}

export async function POST(request: NextRequest) {
  const c = await getClient();
  const { tool, args } = await request.json();
  const result = await c.callTool(tool, args);
  return NextResponse.json(result);
}
```

### Deploy

```bash
vercel deploy
```

## Deno Deploy

### main.ts

```typescript
import init, { McpClient } from 'npm:turbomcp-wasm';

await init();

const client = new McpClient(Deno.env.get('MCP_SERVER_URL')!)
  .withAuth(Deno.env.get('MCP_API_KEY')!);

await client.initialize();

Deno.serve(async (request) => {
  const url = new URL(request.url);

  if (url.pathname === '/tools') {
    const tools = await client.listTools();
    return Response.json(tools);
  }

  if (url.pathname === '/call' && request.method === 'POST') {
    const { tool, args } = await request.json();
    const result = await client.callTool(tool, args);
    return Response.json(result);
  }

  return new Response('Not Found', { status: 404 });
});
```

### Deploy

```bash
deployctl deploy --project=my-mcp-app main.ts
```

## WASI Deployment

### Building for WASI

```bash
# Add WASI target
rustup target add wasm32-wasip2

# Build
cargo build --target wasm32-wasip2 -p your-mcp-app --release
```

### Wasmtime

```bash
wasmtime run \
  --env MCP_SERVER_URL=https://api.example.com/mcp \
  target/wasm32-wasip2/release/your-mcp-app.wasm
```

### WasmEdge

```bash
wasmedge run \
  --env MCP_SERVER_URL=https://api.example.com/mcp \
  target/wasm32-wasip2/release/your-mcp-app.wasm
```

### Spin (Fermyon)

**spin.toml:**

```toml
spin_manifest_version = 2

[application]
name = "mcp-app"
version = "1.0.0"

[[trigger.http]]
route = "/..."
component = "mcp"

[component.mcp]
source = "target/wasm32-wasip2/release/your-mcp-app.wasm"

[component.mcp.build]
command = "cargo build --target wasm32-wasip2 --release"
```

### Deploy to Fermyon Cloud

```bash
spin deploy
```

## Optimizing WASM Size

### Cargo.toml Settings

```toml
[profile.release]
lto = true
opt-level = 'z'  # Optimize for size
codegen-units = 1
panic = 'abort'
strip = true

[profile.release.package."*"]
opt-level = 'z'
```

### Post-build Optimization

```bash
# Install wasm-opt
npm install -g binaryen

# Optimize
wasm-opt -Os -o optimized.wasm output.wasm
```

### Size Comparison

| Configuration | Size |
|--------------|------|
| Debug build | ~5MB |
| Release build | ~1MB |
| Release + LTO | ~500KB |
| + wasm-opt | ~300KB |
| + gzip | ~100KB |

## Caching Strategies

### Client-Side Cache

```javascript
const cache = new Map();

async function cachedCallTool(client, tool, args) {
  const key = JSON.stringify({ tool, args });

  if (cache.has(key)) {
    return cache.get(key);
  }

  const result = await client.callTool(tool, args);
  cache.set(key, result);

  return result;
}
```

### Cloudflare KV Cache

```javascript
export default {
  async fetch(request, env) {
    const cacheKey = `mcp:${request.url}`;

    // Check cache
    const cached = await env.CACHE.get(cacheKey);
    if (cached) {
      return new Response(cached, {
        headers: { 'Content-Type': 'application/json' },
      });
    }

    // Fetch from MCP
    const result = await client.callTool(tool, args);
    const json = JSON.stringify(result);

    // Cache for 5 minutes
    await env.CACHE.put(cacheKey, json, { expirationTtl: 300 });

    return Response.json(result);
  },
};
```

## Error Handling

### Timeout Handling

```javascript
const controller = new AbortController();
const timeout = setTimeout(() => controller.abort(), 25000);

try {
  const result = await client.callTool(tool, args);
  return Response.json(result);
} catch (error) {
  if (error.name === 'AbortError') {
    return new Response('Request timeout', { status: 504 });
  }
  throw error;
} finally {
  clearTimeout(timeout);
}
```

### Retry Logic

```javascript
async function withRetry(fn, retries = 3) {
  for (let i = 0; i < retries; i++) {
    try {
      return await fn();
    } catch (error) {
      if (i === retries - 1) throw error;
      await new Promise(r => setTimeout(r, 1000 * Math.pow(2, i)));
    }
  }
}

const result = await withRetry(() => client.callTool(tool, args));
```

## Monitoring

### Cloudflare Analytics

```javascript
export default {
  async fetch(request, env, ctx) {
    const start = Date.now();

    try {
      const result = await handleRequest(request, env);

      ctx.waitUntil(
        env.ANALYTICS.writeDataPoint({
          indexes: [request.url],
          doubles: [Date.now() - start],
          blobs: ['success'],
        })
      );

      return result;
    } catch (error) {
      ctx.waitUntil(
        env.ANALYTICS.writeDataPoint({
          indexes: [request.url],
          doubles: [Date.now() - start],
          blobs: ['error', error.message],
        })
      );

      throw error;
    }
  },
};
```

## Security Considerations

### API Key Protection

```javascript
// Don't expose API keys to client
// Use edge function as a proxy

export default {
  async fetch(request, env) {
    // Validate request origin
    const origin = request.headers.get('Origin');
    if (!env.ALLOWED_ORIGINS.includes(origin)) {
      return new Response('Forbidden', { status: 403 });
    }

    // Add API key server-side
    const client = new McpClient(env.MCP_SERVER_URL)
      .withAuth(env.MCP_API_KEY);  // Secret, not exposed

    // ...
  },
};
```

### Rate Limiting

```javascript
const RATE_LIMIT = 100;  // requests per minute

export default {
  async fetch(request, env) {
    const ip = request.headers.get('CF-Connecting-IP');
    const key = `rate:${ip}`;

    const count = await env.CACHE.get(key) || 0;
    if (count >= RATE_LIMIT) {
      return new Response('Rate limit exceeded', { status: 429 });
    }

    await env.CACHE.put(key, count + 1, { expirationTtl: 60 });

    // Handle request...
  },
};
```

## Next Steps

- **[WASM Guide](../guide/wasm.md)** - Detailed WASM usage
- **[WASM API](../api/wasm.md)** - Full API reference
- **[Production Setup](production.md)** - Production configuration
- **[Monitoring](monitoring.md)** - Observability setup
