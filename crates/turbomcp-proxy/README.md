# turbomcp-proxy

> **Universal MCP Adapter/Generator** - Introspection, proxying, and code generation for any MCP server

[![MCP Version](https://img.shields.io/badge/MCP-2025--11--25-blue)](https://modelcontextprotocol.io)
[![Rust Version](https://img.shields.io/badge/rustc-1.89+-blue.svg)](https://www.rust-lang.org)

**turbomcp-proxy** is a universal tool that works with **ANY** MCP server implementation (TurboMCP, Python SDK, TypeScript SDK, custom implementations). It discovers server capabilities via the MCP protocol and dynamically generates adapters for different transports and protocols.

---

## Quick Start

```bash
# Inspect any MCP server
turbomcp-proxy inspect --backend stdio --cmd "python my-server.py"

# Expose STDIO server over HTTP/SSE (development)
turbomcp-proxy serve \
  --backend stdio --cmd "python my-server.py" \
  --frontend http --bind 127.0.0.1:3000

# Connect to TCP server and expose over HTTP
turbomcp-proxy serve \
  --backend tcp --tcp localhost:5000 \
  --frontend http --bind 127.0.0.1:3001

# Connect to Unix socket and expose over HTTP
turbomcp-proxy serve \
  --backend unix --unix /tmp/mcp.sock \
  --frontend http --bind 127.0.0.1:3002

# Expose with JWT authentication (production - symmetric)
turbomcp-proxy serve \
  --backend stdio --cmd "python my-server.py" \
  --frontend http --bind 0.0.0.0:3000 \
  --jwt-secret "your-secret-key" \
  --jwt-algorithm HS256 \
  --jwt-audience "https://proxy.example.com/mcp"

# Expose with JWKS (production - asymmetric, OAuth providers)
turbomcp-proxy serve \
  --backend stdio --cmd "python my-server.py" \
  --frontend http --bind 0.0.0.0:3000 \
  --jwt-jwks-uri "https://accounts.google.com/.well-known/jwks.json" \
  --jwt-algorithm RS256 \
  --jwt-audience "https://api.example.com" \
  --jwt-issuer "https://accounts.google.com"

# Generate optimized Rust proxy
turbomcp-proxy generate \
  --backend stdio --cmd "python my-server.py" \
  --frontend http \
  --output ./my-proxy \
  --build --release

# Export OpenAPI 3.1 schema
turbomcp-proxy schema openapi \
  --backend stdio --cmd "python my-server.py" \
  --output api-spec.json

# Export GraphQL schema
turbomcp-proxy schema graphql \
  --backend tcp --tcp localhost:5000 \
  --output schema.graphql

# Export Protobuf definition
turbomcp-proxy schema protobuf \
  --backend unix --unix /tmp/mcp.sock \
  --output server.proto
```

---

## Features

### Universal Compatibility

Works with **any MCP implementation**:
- [x] TurboMCP (Rust)
- [x] Python SDK
- [x] TypeScript SDK
- [x] Custom implementations

### Introspection-Based

- **Zero configuration** - discovers capabilities automatically
- Extracts tools, resources, prompts with JSON schemas
- Caches results for fast repeated use

### Multiple Modes

- **Runtime Mode**: Fast prototyping, no compilation needed
- **Codegen Mode**: Production binaries with 0ms overhead
- **Schema Mode**: Export OpenAPI, GraphQL, Protobuf

### Transport Support

- **Backends:** STDIO (subprocess), Streamable HTTP, TCP, Unix domain sockets, WebSocket
- **Frontends:** Streamable HTTP and STDIO (`serve`), plus WebSocket through `RuntimeProxy`

Every frontend serves the same `ProxyService` through `turbomcp-server`'s own
transports, so the handshake, protocol-version negotiation (2025-06-18 and
2025-11-25), `ping`, and notifications behave exactly as they do for any
TurboMCP server.

### What the proxy relays

The proxy forwards `tools/call`, `resources/read`, and `prompts/get` to the
upstream and serves the upstream's catalogue (tools, resources, resource
templates, prompts, with icons, `_meta`, and annotations intact) from what it
discovered at startup. Upstream errors come back unchanged, `data` included.

It does **not** relay traffic the upstream originates: sampling and
elicitation requests, log messages, progress, and `list_changed` /
`resources/updated` notifications. It therefore advertises only the `tools`,
`resources`, and `prompts` capabilities, with no `listChanged` or `subscribe`,
and no `logging` or `completions`. A catalogue change upstream takes a proxy
restart to show.

### Authentication & Security

- **JWT Authentication** (RFC 7519 validation)
  - Symmetric algorithms: HS256, HS384, HS512
  - Asymmetric algorithms: RS256, RS384, RS512, ES256, ES384
  - JWKS support for OAuth providers (Google, GitHub, Auth0, etc.)
  - Automatic key caching with TTL
  - Claims validation (exp, nbf, iat, iss, aud)
  - Clock skew tolerance (60s default)
  - An audience is required: `--jwt-secret` refuses to start without `--jwt-audience`
  - RFC 9728 protected-resource metadata: when the first audience is the
    proxy's own `https://` URL and an issuer is an `https://` URL, the proxy
    serves the metadata and its 401s point at it (`resource_metadata`)
- **API Key Authentication** (configurable header)
- **Command allowlist** (prevents shell injection)
- **SSRF protection** (blocks private, CGNAT, and metadata addresses in every
  spelling, including IPv4-mapped and NAT64 IPv6)
- **Path traversal protection** (canonical path resolution)
- **Auth token security** (automatic secret zeroization)
- **Request limiting** (DoS protection, 10 MB default)
- **Timeout enforcement** (prevents hanging requests)

---

## Use Cases

### 1. Expose STDIO Server Over HTTP (Most Common Use Case)

**Problem:** You have a CLI MCP server, but need HTTP clients to access it

```bash
# Your CLI server
./my-mcp-server

# Expose it over HTTP (development)
turbomcp-proxy serve \
  --backend stdio --cmd "./my-mcp-server" \
  --frontend http --bind 127.0.0.1:3000

# Expose with JWT authentication (production)
turbomcp-proxy serve \
  --backend stdio --cmd "./my-mcp-server" \
  --frontend http --bind 0.0.0.0:3000 \
  --jwt-secret "your-secret-key" \
  --jwt-audience "https://proxy.example.com/mcp"

# Expose with API key authentication (production)
turbomcp-proxy serve \
  --backend stdio --cmd "./my-mcp-server" \
  --frontend http --bind 0.0.0.0:3000 \
  --require-auth \
  --api-key-header x-api-key

# Now accessible via HTTP
curl -X POST http://localhost:3000/mcp \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <jwt-token>" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

### 2. Connect to HTTP Server from STDIO Client

**Problem:** Your tool expects STDIO, but server is HTTP

```bash
# Connect to HTTP server, expose as STDIO
turbomcp-proxy serve \
  --backend http --http https://api.example.com/mcp \
  --frontend stdio \
  | my-cli-tool

# With backend authentication (Bearer token for HTTP backend)
turbomcp-proxy serve \
  --backend http --http https://api.example.com/mcp \
  --auth-token "your-secret-token" \
  --frontend stdio
```

### 3. Serve an MCP Server as a REST API

**Problem:** Want plain HTTP/JSON endpoints for an MCP server (requires the
`rest` feature: `cargo install turbomcp-proxy --features rest`)

```bash
turbomcp-proxy adapter rest \
  --backend stdio --cmd "python my-server.py" \
  --bind 127.0.0.1:3001

# Endpoints:
#   GET  /api/tools              → the tool catalogue
#   POST /api/tools/{name}       → tools/call, JSON body = arguments
#   GET  /api/resources          → the resource catalogue
#   GET  /api/resources/{uri}    → resources/read (the rest of the path is the URI)
#   GET  /api/prompts            → the prompt catalogue
#   POST /api/prompts/{name}     → prompts/get
#   GET  /openapi.json           → a minimal OpenAPI description
#   GET  /health
```

### 4. Code Generation for Production

**Problem:** Need optimized binary for production deployment

```bash
# Generate standalone Rust project
turbomcp-proxy generate \
  --backend stdio --cmd "python my-server.py" \
  --frontend http \
  --output ./production-proxy \
  --build --release

# Run it: the upstream command comes from the environment
BACKEND_CMD=python BACKEND_ARGS=my-server.py \
  ./production-proxy/target/release/<package-name>
```

The generated crate is an ordinary TurboMCP server: `ProxyRouter` implements
`McpHandler` and is served by `turbomcp-server`'s stdio, Streamable HTTP, or
WebSocket transport (`--frontend`). It routes the tools and prompts it was
generated for by their upstream names, fetches the upstream's catalogue at
startup, and logs to stderr. `BIND_ADDR` sets the listen address for the HTTP
and WebSocket frontends; `BACKEND_WORKING_DIR` sets the upstream's working
directory. Generation currently supports STDIO backends only.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│ Introspection Layer                                     │
│ • McpIntrospector: Discovers server capabilities       │
│ • ServerSpec: Complete server description               │
│ • Backends: STDIO, HTTP, WebSocket                      │
└─────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────┐
│ Generation Layer                                        │
│ • RuntimeProxyBuilder: Dynamic, no codegen              │
│ • RustCodeGenerator: Optimized Rust source              │
│ • Schema Generators: OpenAPI, GraphQL, Protobuf         │
└─────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────┐
│ Adapter Layer                                           │
│ • Transport Adapters: STDIO ↔ HTTP/SSE ↔ WebSocket     │
│ • Protocol Adapters: MCP → REST API / GraphQL          │
└─────────────────────────────────────────────────────────┘
```

---

## Installation

**From crates.io:**
```bash
cargo install turbomcp-proxy
```

**From source:**
```bash
cd crates/turbomcp-proxy
cargo install --path .
```

---

## Documentation

- **[Examples](./examples/)** — Runnable usage examples
- **[API Docs](https://docs.rs/turbomcp-proxy)** — Rust API documentation

---

## CLI Reference

### Commands

```
turbomcp-proxy <COMMAND> [OPTIONS]

Commands:
  inspect   Discover MCP server capabilities
  serve     Run runtime proxy (no codegen)
  generate  Generate optimized proxy source code
  schema    Export schemas (OpenAPI, GraphQL, Protobuf)
  adapter   Run protocol adapter (MCP → REST/GraphQL)
  help      Print help
```

### `inspect` - Discover Capabilities

```bash
turbomcp-proxy inspect [OPTIONS]

Backend Options:
  --backend <TYPE>    Backend type (stdio, http, tcp, unix, websocket)
  --cmd <CMD>         Command to run (for stdio backend)
  --args <ARGS>       Command arguments (for stdio, repeatable)
  --http <URL>        HTTP/SSE server URL
  --tcp <ADDR>        TCP endpoint (host:port)
  --unix <PATH>       Unix socket path
  --websocket <URL>   WebSocket server URL

Output Options:
  -o, --output <FILE> Save to file
  --append            Append to output file instead of overwriting

Global (before the subcommand name):
  -f, --format <FMT>  Output format: human (default), json, yaml

Examples:
  turbomcp-proxy inspect --backend stdio --cmd "python server.py"
  turbomcp-proxy -f json inspect --backend stdio --cmd "python server.py"
  turbomcp-proxy inspect --backend stdio --cmd node --args dist/server.js -o spec.json

Note: Inspect currently supports only the STDIO backend. The other backends
(HTTP, TCP, Unix, WebSocket) are fully wired in `serve` and `schema`, but the
inspect command returns a configuration error for them today.
```

### `serve` - Runtime Proxy

```bash
turbomcp-proxy serve [OPTIONS]

Backend Options:
  --backend <TYPE>    Backend type (stdio, http, tcp, unix, websocket)
  --cmd <CMD>         Command to run (for stdio backend)
  --args <ARGS>       Command arguments (for stdio backend, repeatable)
  --working-dir <DIR> Working directory for subprocess (for stdio backend)
  --http <URL>        HTTP/SSE server URL (for http backend)
  --websocket <URL>   WebSocket server URL (for websocket backend)
  --tcp <HOST:PORT>   TCP endpoint (for tcp backend)
  --unix <PATH>       Unix socket path (for unix backend)
  --auth-token <TOK>  Bearer token for HTTP backend authentication

Frontend Options:
  --frontend <TYPE>   Frontend type: http (default) or stdio
  --bind <ADDR>       Bind address (default: 127.0.0.1:3000)
  --path <PATH>       HTTP endpoint path (default: /mcp)

Authentication Options (Frontend HTTP Server):
  --jwt-secret <SECRET>        JWT secret (symmetric HS256/384/512)
  --jwt-jwks-uri <URI>         JWKS URI for asymmetric RS*/ES* validation
  --jwt-algorithm <ALG>        JWT algorithm (default: HS256)
  --jwt-audience <AUD>         Required `aud` claim (repeatable; required with any JWT option)
  --jwt-issuer <ISS>           Required `iss` claim (repeatable)
  --api-key-header <HEADER>    API key header name (default: x-api-key)
  --require-auth               Require authentication for all requests

Environment Variables:
  TURBOMCP_JWT_SECRET          Alternative to --jwt-secret
  TURBOMCP_JWT_JWKS_URI        Alternative to --jwt-jwks-uri

Examples:
  # STDIO → HTTP (development, localhost only)
  turbomcp-proxy serve \
    --backend stdio --cmd "python server.py" \
    --frontend http --bind 127.0.0.1:3000

  # STDIO → HTTP with JWT authentication (production)
  turbomcp-proxy serve \
    --backend stdio --cmd "python server.py" \
    --frontend http --bind 0.0.0.0:3000 \
    --jwt-secret "your-secret-key" \
    --jwt-audience "https://proxy.example.com/mcp"

  # STDIO → HTTP with API key authentication (production)
  turbomcp-proxy serve \
    --backend stdio --cmd "python server.py" \
    --frontend http --bind 0.0.0.0:3000 \
    --require-auth

  # HTTP → STDIO with backend authentication
  turbomcp-proxy serve \
    --backend http --http https://api.example.com/mcp \
    --auth-token "backend-token" \
    --frontend stdio

  # TCP → HTTP (high-performance network)
  turbomcp-proxy serve \
    --backend tcp --tcp localhost:5000 \
    --frontend http --bind 0.0.0.0:3000

  # Unix socket → HTTP (IPC security)
  turbomcp-proxy serve \
    --backend unix --unix /tmp/mcp.sock \
    --frontend http --bind 0.0.0.0:3000
```

### `generate` - Code Generation

```bash
turbomcp-proxy generate [OPTIONS]

Options:
  --backend <TYPE>    Backend type (stdio, http, tcp, unix, websocket)
  --cmd <CMD>         Command to run (for stdio backend)
  --args <ARGS>       Command arguments (for stdio backend, repeatable)
  --http <URL>        HTTP/SSE server URL (for http backend)
  --websocket <URL>   WebSocket server URL (for websocket backend)
  --tcp <HOST:PORT>   TCP endpoint (for tcp backend)
  --unix <PATH>       Unix socket path (for unix backend)
  --frontend <TYPE>   Frontend type (default: http)
  --output, -o <DIR>  Output directory (required)
  --name <NAME>       Package name (defaults to server name)
  --version <VER>     Package version (default: 0.1.0)
  --build             Build after generation
  --release           Build in release mode (requires --build)
  --run               Run after building (requires --build)

Examples:
  # Generate and build
  turbomcp-proxy generate \
    --backend stdio --cmd "python server.py" \
    --frontend http \
    --output ./my-proxy \
    --build --release
```

### `schema` - Schema Export

Export MCP server capabilities as standard schema formats.

```bash
turbomcp-proxy schema <FORMAT> [OPTIONS]

Formats:
  openapi       OpenAPI 3.1 specification (REST API schema)
  graphql       GraphQL Schema Definition Language
  protobuf      Protocol Buffers 3 definition

Backend Options:
  --backend <TYPE>    Backend type (stdio, http, tcp, unix, websocket)
  --cmd <CMD>         Command to run (for stdio backend)
  --http <URL>        HTTP/SSE server URL
  --tcp <ADDR>        TCP endpoint (host:port)
  --unix <PATH>       Unix socket path

Output Options:
  --output <FILE>     Output file (default: stdout)
  --with-examples     Include example requests/responses (OpenAPI only)

Examples:
  # Export OpenAPI from STDIO server
  turbomcp-proxy schema openapi \
    --backend stdio --cmd "python server.py" \
    --output api-spec.json

  # Export GraphQL from TCP server
  turbomcp-proxy schema graphql \
    --backend tcp --tcp localhost:5000 \
    --output schema.graphql

  # Export Protobuf from Unix socket
  turbomcp-proxy schema protobuf \
    --backend unix --unix /tmp/mcp.sock \
    --output server.proto

  # Export to stdout
  turbomcp-proxy schema openapi \
    --backend stdio --cmd "npx @mcp/server-fs /tmp"
```

### `adapter` - Protocol Adapters

Expose MCP servers through standard web protocols. `rest` needs the `rest`
feature; `graphql` is not implemented and returns an error.

```bash
turbomcp-proxy adapter <PROTOCOL> [OPTIONS]

Protocols:
  rest        REST API with OpenAPI documentation
  graphql     GraphQL API with schema explorer

Backend Options:
  --backend <TYPE>    Backend type (stdio, http, tcp, unix, websocket)
  --cmd <CMD>         Command to run (for stdio backend)
  --http <URL>        HTTP/SSE server URL
  --tcp <ADDR>        TCP endpoint (host:port)
  --unix <PATH>       Unix socket path

Server Options:
  --bind <ADDR>       Bind address (default: 127.0.0.1:3001)

REST-Specific:
  --openapi-ui        Accepted; no Swagger UI is served yet

Examples:
  turbomcp-proxy adapter rest \
    --backend stdio --cmd "python server.py" \
    --bind 127.0.0.1:3001
```

---

## Development Status

**Current Version:** 3.5.0 (tracks the TurboMCP workspace)
**Transport Coverage:**
- [x] **Backends:** STDIO, Streamable HTTP, TCP, Unix domain sockets, WebSocket
- [x] **Frontends:** Streamable HTTP, STDIO, WebSocket (`RuntimeProxy`)
- [ ] **TCP / Unix frontends:** `TcpFrontend` and `UnixFrontend` accept
  connections but do not route requests yet

**Authentication & Security:**
- [x] **JWT Authentication** (RFC 7519, symmetric and JWKS validation)
- [x] **API Key Authentication** (configurable header and key)
- [x] **Environment Variable Support** (TURBOMCP_JWT_SECRET)
- [x] **Security Warnings** (alerts when binding publicly without auth)
- [x] **Command Allowlist** (prevents shell injection)
- [x] **SSRF Protection** (blocks private IPs, metadata endpoints)
- [x] **Path Traversal Protection** (canonical path resolution)
- [x] **Auth Token Security** (automatic secret zeroization)

**Quality Assurance:**
- [x] **40+ Comprehensive Tests** (transport combinations, security validations)
- [x] **Security-Focused Regression Coverage**
- [x] **Zero TODO Markers** (production-ready)
- [x] **100% Safe Rust** (no unsafe code)

**Core Components:**
- [x] **BackendConnector**: Supports 5 transport types with type-erased enum dispatch
- [x] **ProxyService**: `turbomcp-server` handler for Streamable HTTP integration
- [x] **IdTranslator**: Bidirectional message ID mapping for session correlation
- [x] **Introspection**: Complete server capability discovery (tools, resources, prompts)
- [x] **RuntimeProxyBuilder**: Security-first builder with comprehensive validation
- [x] **Authentication**: JWT and API key support in the proxy frontend

### What's not done yet

- **Server-to-client relay** — sampling, elicitation, logging, progress, and
  change notifications from the upstream are not forwarded (see "What the
  proxy relays").
- **Per-user backend identity** — the frontend credential is checked, but
  every client shares one backend connection that authenticates as the proxy
  (`--auth-token`). `JwtSigner` exists as a building block and is not wired
  into `serve`.
- **Swagger UI for the REST adapter** — `--openapi-ui` is accepted but serves
  nothing yet.
- **Full GraphQL adapter** — scaffolded behind the `graphql` feature flag;
  no `async-graphql` dependency is pinned yet, so the feature on its own
  does not produce a working adapter.
- **Inspect over non-STDIO backends** — the `inspect` command rejects HTTP,
  TCP, Unix, and WebSocket today. Use `schema` (which does support all
  backends) if you need introspection output over those transports.

---

## Contributing

Contributions welcome through the top-level TurboMCP repository.

---

## License

Licensed under MIT.

---

## Why turbomcp-proxy?

### Problem
MCP servers are often CLI tools (STDIO), but clients need network access (HTTP). Manually bridging this gap requires:
- Writing transport code
- Handling sessions
- Mapping message IDs
- Writing schemas/docs

### Solution
**turbomcp-proxy** does this automatically via introspection:
1. **Connect** to any MCP server
2. **Discover** capabilities via protocol
3. **Generate** adapters dynamically or statically
4. **Expose** over any transport/protocol

**Result:** Zero-configuration, universal MCP adapter that works with any implementation.

---

**Built by the TurboMCP team**
